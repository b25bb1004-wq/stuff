//! Blocking TCP client for a Varve database server.

use std::io;
use std::net::{SocketAddr, TcpStream};

use crate::net::codec::{decode_rows, read_frame, write_frame};
use crate::sql::Rows;

/// TCP client connected to a Varve [`Server`](super::Server).
pub struct Client {
    stream: TcpStream,
}

impl Client {
    /// Connect to a Varve server at `addr`.
    pub fn connect(addr: SocketAddr) -> io::Result<Client> {
        let stream = TcpStream::connect(addr)?;
        Ok(Client { stream })
    }

    /// Execute one SQL statement and return its rows or a server error message.
    pub fn execute(&mut self, sql: &str) -> io::Result<Result<Rows, String>> {
        write_frame(&mut self.stream, sql.as_bytes())?;
        let reply = read_frame(&mut self.stream)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "server closed connection")
        })?;
        let (&tag, rest) = reply
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty reply frame"))?;
        match tag {
            0x00 => {
                let rows = decode_rows(rest).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid rows payload")
                })?;
                Ok(Ok(rows))
            }
            0x01 => {
                let msg = std::str::from_utf8(rest).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid utf-8 error message")
                })?;
                Ok(Err(msg.to_owned()))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown reply tag",
            )),
        }
    }
}
