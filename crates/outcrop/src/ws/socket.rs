//! Blocking server-side WebSocket connection over `TcpStream`.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::string::String;
use std::vec::Vec;

use super::frame::{decode_frame, encode_frame, Frame, WsError};
use super::Message;

const DEFAULT_MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// An upgraded WebSocket connection owned by a route handler.
pub struct WebSocket {
    stream: TcpStream,
    read_buf: Vec<u8>,
    max_message: usize,
    closed: bool,
}

impl WebSocket {
    /// Wrap an upgraded `TcpStream` and any already-buffered bytes from the HTTP read loop.
    pub(crate) fn new(stream: TcpStream, read_buf: Vec<u8>) -> Self {
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_nodelay(true);
        Self {
            stream,
            read_buf,
            max_message: DEFAULT_MAX_MESSAGE,
            closed: false,
        }
    }

    /// Next data message (fragments reassembled). Pings are answered with a pong carrying
    /// the same payload and not returned; pongs are skipped. A close frame is echoed and
    /// returned as `Message::Close`; later calls give `Err(Closed)`.
    pub fn recv(&mut self) -> Result<Message, WsError> {
        if self.closed {
            return Err(WsError::Closed);
        }

        let mut frag_opcode: Option<u8> = None;
        let mut frag_buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 8192];

        loop {
            match decode_frame(&self.read_buf, true, self.max_message) {
                Ok(Some((frame, used))) => {
                    self.read_buf.drain(..used);
                    match frame.opcode {
                        0x9 => {
                            let pong = Frame {
                                fin: true,
                                opcode: 0xA,
                                payload: frame.payload,
                            };
                            self.write_frame(&pong)?;
                        }
                        0xA => {}
                        0x8 => {
                            return self.handle_close(frame.payload);
                        }
                        0x0 => {
                            let Some(start_op) = frag_opcode else {
                                self.fail_close(1002);
                                return Err(WsError::Protocol);
                            };
                            if frag_buf.len().saturating_add(frame.payload.len()) > self.max_message
                            {
                                self.fail_close(1009);
                                return Err(WsError::TooBig);
                            }
                            frag_buf.extend_from_slice(&frame.payload);
                            if frame.fin {
                                let data = core::mem::take(&mut frag_buf);
                                return self.finish_data(start_op, data);
                            }
                        }
                        0x1 | 0x2 => {
                            if frag_opcode.is_some() {
                                self.fail_close(1002);
                                return Err(WsError::Protocol);
                            }
                            if frame.fin {
                                return self.finish_data(frame.opcode, frame.payload);
                            }
                            frag_opcode = Some(frame.opcode);
                            frag_buf = frame.payload;
                        }
                        _ => {
                            self.fail_close(1002);
                            return Err(WsError::Protocol);
                        }
                    }
                }
                Ok(None) => match self.stream.read(&mut tmp) {
                    Ok(0) => {
                        self.closed = true;
                        return Err(WsError::Closed);
                    }
                    Ok(n) => {
                        self.read_buf.extend_from_slice(&tmp[..n]);
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => {
                        self.closed = true;
                        return Err(WsError::Io);
                    }
                },
                Err(WsError::Protocol) => {
                    self.fail_close(1002);
                    return Err(WsError::Protocol);
                }
                Err(WsError::TooBig) => {
                    self.fail_close(1009);
                    return Err(WsError::TooBig);
                }
                Err(err) => {
                    self.fail_close(1002);
                    return Err(err);
                }
            }
        }
    }

    /// Send `m` as a single unmasked WebSocket frame.
    pub fn send(&mut self, m: &Message) -> Result<(), WsError> {
        if self.closed {
            return Err(WsError::Closed);
        }
        let frame = match m {
            Message::Text(s) => Frame {
                fin: true,
                opcode: 0x1,
                payload: s.as_bytes().to_vec(),
            },
            Message::Binary(b) => Frame {
                fin: true,
                opcode: 0x2,
                payload: b.clone(),
            },
            Message::Ping(b) => Frame {
                fin: true,
                opcode: 0x9,
                payload: b.clone(),
            },
            Message::Pong(b) => Frame {
                fin: true,
                opcode: 0xA,
                payload: b.clone(),
            },
            Message::Close(opt) => {
                let mut payload = Vec::new();
                if let Some((code, reason)) = opt {
                    payload.extend_from_slice(&code.to_be_bytes());
                    payload.extend_from_slice(reason.as_bytes());
                }
                Frame {
                    fin: true,
                    opcode: 0x8,
                    payload,
                }
            }
        };
        self.write_frame(&frame)?;
        if matches!(m, Message::Close(_)) {
            self.closed = true;
            let _ = self.stream.shutdown(Shutdown::Write);
        }
        Ok(())
    }

    fn finish_data(&mut self, opcode: u8, data: Vec<u8>) -> Result<Message, WsError> {
        if opcode == 0x1 {
            let Ok(text) = String::from_utf8(data) else {
                self.fail_close(1007);
                return Err(WsError::Utf8);
            };
            Ok(Message::Text(text))
        } else {
            Ok(Message::Binary(data))
        }
    }

    fn handle_close(&mut self, payload: Vec<u8>) -> Result<Message, WsError> {
        if payload.len() == 1 {
            self.fail_close(1002);
            return Err(WsError::Protocol);
        }
        if payload.is_empty() {
            let _ = self.write_frame(&Frame {
                fin: true,
                opcode: 0x8,
                payload: Vec::new(),
            });
            let _ = self.stream.shutdown(Shutdown::Both);
            self.closed = true;
            return Ok(Message::Close(None));
        }
        let code = u16::from_be_bytes([payload[0], payload[1]]);
        let Ok(reason) = String::from_utf8(payload[2..].to_vec()) else {
            self.fail_close(1007);
            return Err(WsError::Utf8);
        };
        let _ = self.write_frame(&Frame {
            fin: true,
            opcode: 0x8,
            payload,
        });
        let _ = self.stream.shutdown(Shutdown::Both);
        self.closed = true;
        Ok(Message::Close(Some((code, reason))))
    }

    fn write_frame(&mut self, frame: &Frame) -> Result<(), WsError> {
        let wire = encode_frame(frame, None);
        self.stream.write_all(&wire).map_err(|_| WsError::Io)?;
        self.stream.flush().map_err(|_| WsError::Io)?;
        Ok(())
    }

    fn fail_close(&mut self, code: u16) {
        let frame = Frame {
            fin: true,
            opcode: 0x8,
            payload: code.to_be_bytes().to_vec(),
        };
        let _ = self.write_frame(&frame);
        let _ = self.stream.shutdown(Shutdown::Both);
        self.closed = true;
    }
}
