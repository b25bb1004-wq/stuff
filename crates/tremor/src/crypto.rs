//! Handshake key derivation and packet AEAD wrappers for Tremor.

use crate::wire::{
    nonce, Header, Params, HEADER_LEN, INITIAL_PLAINTEXT_LEN, PKT_DATA, PKT_INITIAL, PKT_RESPONSE,
    VERSION,
};
use crate::MAX_DATAGRAM;
use onyx::{aead, hkdf, sha256::Sha256, x25519};

const DOMAIN: &[u8] = b"tremor/1";

/// Keys and transcript hash produced by a completed handshake.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionKeys {
    pub(crate) h1: [u8; 32],
    pub(crate) k_c2s: [u8; 32],
    pub(crate) k_s2c: [u8; 32],
}

/// Client-side handshake secrets kept until Response is opened.
#[derive(Debug, Clone)]
pub(crate) struct ClientHandshake {
    pub(crate) e_c: [u8; 32],
    pub(crate) h0: [u8; 32],
    pub(crate) prk1: [u8; 32],
    pub(crate) initial_datagram: Vec<u8>,
}

fn expand32(prk: &[u8; 32], label: &[u8], ctx: &[u8; 32]) -> Option<[u8; 32]> {
    let mut info = Vec::with_capacity(label.len() + 32);
    info.extend_from_slice(label);
    info.extend_from_slice(ctx);
    let mut out = [0u8; 32];
    hkdf::expand(prk, &info, &mut out).ok()?;
    Some(out)
}

fn compute_h0(server_pub: &[u8; 32], pub_e_c: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(server_pub);
    h.update(pub_e_c);
    h.finalize()
}

fn compute_h1(h0: &[u8; 32], initial_datagram: &[u8], pub_e_s: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(h0);
    h.update(initial_datagram);
    h.update(pub_e_s);
    h.finalize()
}

/// Build the client's Initial datagram and handshake state. Returns `None` if `server_pub` is weak.
pub(crate) fn build_initial(
    e_c: [u8; 32],
    cid: u64,
    server_pub: &[u8; 32],
    params: Params,
) -> Option<ClientHandshake> {
    let pub_e_c = x25519::public_key(&e_c);
    let es = x25519::shared_secret(&e_c, server_pub).ok()?;
    let h0 = compute_h0(server_pub, &pub_e_c);
    let prk1 = hkdf::extract(DOMAIN, &es);
    let k_init = expand32(&prk1, b"init", &h0)?;

    let header = Header {
        pkt_type: PKT_INITIAL,
        version: VERSION,
        cid,
        pn: 0,
    }
    .encode();

    let mut aad = [0u8; HEADER_LEN + 32];
    aad[..HEADER_LEN].copy_from_slice(&header);
    aad[HEADER_LEN..].copy_from_slice(&pub_e_c);

    let mut pt = Vec::with_capacity(INITIAL_PLAINTEXT_LEN);
    params.encode(&mut pt);
    if pt.len() > INITIAL_PLAINTEXT_LEN {
        return None;
    }
    pt.resize(INITIAL_PLAINTEXT_LEN, 0);

    let sealed = aead::seal(&k_init, &nonce(0), &aad, &pt);
    let mut initial_datagram = Vec::with_capacity(MAX_DATAGRAM);
    initial_datagram.extend_from_slice(&aad);
    initial_datagram.extend_from_slice(&sealed);

    Some(ClientHandshake {
        e_c,
        h0,
        prk1,
        initial_datagram,
    })
}

/// Decoded and verified client Initial on the server.
pub(crate) struct OpenedInitial {
    pub(crate) cid: u64,
    pub(crate) peer_params: Params,
    pub(crate) h0: [u8; 32],
    pub(crate) prk1: [u8; 32],
    pub(crate) pub_e_c: [u8; 32],
}

/// Verify and open a 1200-byte Initial datagram on the server.
pub(crate) fn open_initial(
    server_secret: &[u8; 32],
    server_pub: &[u8; 32],
    datagram: &[u8],
) -> Option<OpenedInitial> {
    if datagram.len() != MAX_DATAGRAM {
        return None;
    }
    let hdr = Header::decode(datagram)?;
    if hdr.pkt_type != PKT_INITIAL || hdr.version != VERSION || hdr.pn != 0 {
        return None;
    }
    let pub_e_c: [u8; 32] = datagram.get(HEADER_LEN..HEADER_LEN + 32)?.try_into().ok()?;
    let es = x25519::shared_secret(server_secret, &pub_e_c).ok()?;
    let h0 = compute_h0(server_pub, &pub_e_c);
    let prk1 = hkdf::extract(DOMAIN, &es);
    let k_init = expand32(&prk1, b"init", &h0)?;
    let aad = datagram.get(..HEADER_LEN + 32)?;
    let sealed = datagram.get(HEADER_LEN + 32..)?;
    let pt = aead::open(&k_init, &nonce(0), aad, sealed).ok()?;
    let peer_params = Params::decode(&pt, true)?;
    Some(OpenedInitial {
        cid: hdr.cid,
        peer_params,
        h0,
        prk1,
        pub_e_c,
    })
}

/// Derive session keys and build the Response datagram on the server.
pub(crate) fn build_response(
    opened: &OpenedInitial,
    initial_datagram: &[u8],
    e_s: [u8; 32],
    local_params: Params,
) -> Option<(SessionKeys, Vec<u8>)> {
    let pub_e_s = x25519::public_key(&e_s);
    let ee = x25519::shared_secret(&e_s, &opened.pub_e_c).ok()?;
    let h1 = compute_h1(&opened.h0, initial_datagram, &pub_e_s);
    let prk2 = hkdf::extract(&opened.prk1, &ee);
    let k_c2s = expand32(&prk2, b"c2s", &h1)?;
    let k_s2c = expand32(&prk2, b"s2c", &h1)?;

    let header = Header {
        pkt_type: PKT_RESPONSE,
        version: VERSION,
        cid: opened.cid,
        pn: 0,
    }
    .encode();

    let mut aad = [0u8; HEADER_LEN + 32];
    aad[..HEADER_LEN].copy_from_slice(&header);
    aad[HEADER_LEN..].copy_from_slice(&pub_e_s);

    let mut pt = Vec::new();
    local_params.encode(&mut pt);
    let sealed = aead::seal(&k_s2c, &nonce(0), &aad, &pt);

    let mut datagram = Vec::with_capacity(aad.len() + sealed.len());
    datagram.extend_from_slice(&aad);
    datagram.extend_from_slice(&sealed);

    Some((SessionKeys { h1, k_c2s, k_s2c }, datagram))
}

/// Verify and open a Response datagram on the client.
pub(crate) fn open_response(
    hs: &ClientHandshake,
    expected_cid: u64,
    datagram: &[u8],
) -> Option<(SessionKeys, Params)> {
    if datagram.len() > MAX_DATAGRAM {
        return None;
    }
    let hdr = Header::decode(datagram)?;
    if hdr.pkt_type != PKT_RESPONSE
        || hdr.version != VERSION
        || hdr.cid != expected_cid
        || hdr.pn != 0
    {
        return None;
    }
    let pub_e_s: [u8; 32] = datagram.get(HEADER_LEN..HEADER_LEN + 32)?.try_into().ok()?;
    let ee = x25519::shared_secret(&hs.e_c, &pub_e_s).ok()?;
    let h1 = compute_h1(&hs.h0, &hs.initial_datagram, &pub_e_s);
    let prk2 = hkdf::extract(&hs.prk1, &ee);
    let k_c2s = expand32(&prk2, b"c2s", &h1)?;
    let k_s2c = expand32(&prk2, b"s2c", &h1)?;
    let aad = datagram.get(..HEADER_LEN + 32)?;
    let sealed = datagram.get(HEADER_LEN + 32..)?;
    let pt = aead::open(&k_s2c, &nonce(0), aad, sealed).ok()?;
    let peer_params = Params::decode(&pt, false)?;
    Some((SessionKeys { h1, k_c2s, k_s2c }, peer_params))
}

/// Seal a Data packet with `key` and packet number `pn`.
pub(crate) fn seal_data(key: &[u8; 32], cid: u64, pn: u64, plaintext: &[u8]) -> Vec<u8> {
    let header = Header {
        pkt_type: PKT_DATA,
        version: VERSION,
        cid,
        pn,
    }
    .encode();
    let sealed = aead::seal(key, &nonce(pn), &header, plaintext);
    let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&sealed);
    out
}

/// Open a Data packet after header checks pass.
pub(crate) fn open_data(key: &[u8; 32], datagram: &[u8], pn: u64) -> Option<Vec<u8>> {
    let aad = datagram.get(..HEADER_LEN)?;
    let sealed = datagram.get(HEADER_LEN..)?;
    aead::open(key, &nonce(pn), aad, sealed).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::Config;

    #[test]
    fn handshake_crypto_round_trip() {
        let server_secret = [7u8; 32];
        let server_pub = x25519::public_key(&server_secret);
        let e_c = [11u8; 32];
        let e_s = [13u8; 32];
        let params = Params::from_config(&Config::default());

        let hs = build_initial(e_c, 0x1234, &server_pub, params).unwrap();
        assert_eq!(hs.initial_datagram.len(), MAX_DATAGRAM);

        let opened = open_initial(&server_secret, &server_pub, &hs.initial_datagram).unwrap();
        assert_eq!(opened.cid, 0x1234);
        assert_eq!(opened.peer_params, params);

        let (s_keys, resp) = build_response(&opened, &hs.initial_datagram, e_s, params).unwrap();
        let (c_keys, c_peer_params) = open_response(&hs, 0x1234, &resp).unwrap();
        assert_eq!(c_peer_params, params);
        assert_eq!(s_keys.h1, c_keys.h1);
        assert_eq!(s_keys.k_c2s, c_keys.k_c2s);
        assert_eq!(s_keys.k_s2c, c_keys.k_s2c);
    }
}
