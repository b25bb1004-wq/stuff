//! Signed binary package format (`CAIRNPKG1` / `CAIRNSIG1`).

use std::fmt;

use onyx::ed25519;

use crate::manifest::Manifest;

const BODY_MAGIC: &[u8; 10] = b"CAIRNPKG1\n";
const SIG_MAGIC: &[u8; 10] = b"CAIRNSIG1\n";
const SIG_HEADER_LEN: usize = 10 + 32 + 64;

/// A package containing its manifest and relative file entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Package manifest.
    pub manifest: Manifest,
    /// Package files as `(relative_path, contents)` pairs.
    pub files: Vec<(String, Vec<u8>)>,
}

/// Errors returned when encoding, decoding, signing, or opening a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PkgError {
    /// Package bytes or structure are malformed.
    Malformed(String),
    /// A file path violates package path rules.
    BadPath(String),
    /// The Ed25519 signature failed verification.
    BadSignature,
    /// The signing public key is not in the trusted key list.
    UntrustedKey,
}

impl fmt::Display for PkgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PkgError::Malformed(msg) => write!(f, "malformed package: {msg}"),
            PkgError::BadPath(path) => write!(f, "invalid package path: '{path}'"),
            PkgError::BadSignature => write!(f, "invalid package signature"),
            PkgError::UntrustedKey => write!(f, "untrusted package signing key"),
        }
    }
}

impl std::error::Error for PkgError {}

pub(crate) fn validate_path(path: &str) -> Result<(), PkgError> {
    if path.is_empty() || path.len() > 255 || path.starts_with('/') {
        return Err(PkgError::BadPath(path.to_string()));
    }
    if path.bytes().any(|b| b == b'\\' || b == b':' || b == 0) || path.chars().any(char::is_control)
    {
        return Err(PkgError::BadPath(path.to_string()));
    }
    for comp in path.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(PkgError::BadPath(path.to_string()));
        }
    }
    Ok(())
}

/// Encode a package into its canonical unsigned body bytes (`CAIRNPKG1\n`).
pub fn encode(p: &Package) -> Vec<u8> {
    let mut entries: Vec<(&str, &[u8])> = Vec::with_capacity(p.files.len());
    for (path, data) in &p.files {
        assert!(validate_path(path).is_ok(), "invalid package path: {path}");
        entries.push((path.as_str(), data.as_slice()));
    }
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    for pair in entries.windows(2) {
        assert!(
            pair[0].0 != pair[1].0,
            "duplicate package path: {}",
            pair[0].0
        );
    }

    let manifest_text = p.manifest.to_string();
    let manifest_len = u32::try_from(manifest_text.len()).expect("manifest too large");
    let count = u32::try_from(entries.len()).expect("too many package files");

    let total_data: usize = entries.iter().map(|(k, v)| 2 + k.len() + 4 + v.len()).sum();
    let mut out = Vec::with_capacity(10 + 4 + manifest_text.len() + 4 + total_data);
    out.extend_from_slice(BODY_MAGIC);
    out.extend_from_slice(&manifest_len.to_le_bytes());
    out.extend_from_slice(manifest_text.as_bytes());
    out.extend_from_slice(&count.to_le_bytes());

    for (path, data) in entries {
        let plen = u16::try_from(path.len()).expect("path length fits in u16");
        let dlen = u32::try_from(data.len()).expect("file data too large");
        out.extend_from_slice(&plen.to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&dlen.to_le_bytes());
        out.extend_from_slice(data);
    }
    out
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], PkgError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| PkgError::Malformed("length overflow".to_string()))?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| PkgError::Malformed("unexpected end of package".to_string()))?;
        self.pos = end;
        Ok(slice)
    }

    fn read_u16(&mut self) -> Result<u16, PkgError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, PkgError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Decode a package from its unsigned body bytes (`CAIRNPKG1\n`).
pub fn decode(body: &[u8]) -> Result<Package, PkgError> {
    let mut cur = Cursor { buf: body, pos: 0 };
    let magic = cur.take(BODY_MAGIC.len())?;
    if magic != BODY_MAGIC {
        return Err(PkgError::Malformed("bad package magic".to_string()));
    }

    let manifest_len = cur.read_u32()? as usize;
    let manifest_bytes = cur.take(manifest_len)?;
    let manifest_str = std::str::from_utf8(manifest_bytes)
        .map_err(|_| PkgError::Malformed("manifest is not valid UTF-8".to_string()))?;
    let manifest = Manifest::parse(manifest_str)
        .map_err(|e| PkgError::Malformed(format!("invalid manifest: {e}")))?;
    if manifest.to_string() != manifest_str {
        return Err(PkgError::Malformed(
            "manifest is not in canonical form".to_string(),
        ));
    }

    let count = cur.read_u32()? as usize;
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(count.min(1024));
    let mut prev_path: Option<String> = None;

    for _ in 0..count {
        let plen = cur.read_u16()? as usize;
        let path_bytes = cur.take(plen)?;
        let path = std::str::from_utf8(path_bytes)
            .map_err(|_| PkgError::Malformed("file path is not valid UTF-8".to_string()))?;
        validate_path(path)?;
        if let Some(ref prev) = prev_path {
            if path.as_bytes() <= prev.as_bytes() {
                return Err(PkgError::Malformed(
                    "package paths are not strictly increasing".to_string(),
                ));
            }
        }
        let dlen = cur.read_u32()? as usize;
        let data = cur.take(dlen)?;
        prev_path = Some(path.to_string());
        files.push((path.to_string(), data.to_vec()));
    }

    if cur.pos != body.len() {
        return Err(PkgError::Malformed(
            "trailing bytes after package body".to_string(),
        ));
    }

    Ok(Package { manifest, files })
}

/// Encode and sign a package with an Ed25519 `seed`, returning a `CAIRNSIG1\n` blob.
pub fn sign(p: &Package, seed: &[u8; 32]) -> Vec<u8> {
    let body = encode(p);
    let pk = ed25519::public_key(seed);
    let sig = ed25519::sign(seed, &body);
    let mut blob = Vec::with_capacity(SIG_HEADER_LEN + body.len());
    blob.extend_from_slice(SIG_MAGIC);
    blob.extend_from_slice(&pk);
    blob.extend_from_slice(&sig);
    blob.extend_from_slice(&body);
    blob
}

/// Verify and decode a signed package blob against `trusted` public keys.
pub fn open(blob: &[u8], trusted: &[[u8; 32]]) -> Result<Package, PkgError> {
    if blob.len() < SIG_HEADER_LEN || !blob.starts_with(SIG_MAGIC) {
        return Err(PkgError::Malformed(
            "missing signature header or truncated blob".to_string(),
        ));
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&blob[10..42]);
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&blob[42..106]);
    let body = &blob[106..];

    if !trusted.iter().any(|k| k == &pk) {
        return Err(PkgError::UntrustedKey);
    }
    if ed25519::verify(&pk, body, &sig).is_err() {
        return Err(PkgError::BadSignature);
    }
    decode(body)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::version::{Req, Version};

    fn sample_pkg() -> Package {
        Package {
            manifest: Manifest {
                name: "hello".to_string(),
                version: Version::parse("1.2.3").unwrap(),
                dependencies: vec![("dep".to_string(), Req::parse("^1.0").unwrap())],
            },
            files: vec![
                ("z/last.txt".to_string(), b"last".to_vec()),
                ("a/first.bin".to_string(), vec![0, 1, 2, 3]),
            ],
        }
    }

    #[test]
    fn encode_decode_roundtrip_sorts_paths() {
        let pkg = sample_pkg();
        let body = encode(&pkg);
        let decoded = decode(&body).unwrap();
        assert_eq!(decoded.manifest, pkg.manifest);
        assert_eq!(
            decoded.files,
            vec![
                ("a/first.bin".to_string(), vec![0, 1, 2, 3]),
                ("z/last.txt".to_string(), b"last".to_vec()),
            ]
        );
        assert_eq!(encode(&decoded), body);
    }

    #[test]
    fn sign_open_roundtrip_and_error_order() {
        let seed = [0x11u8; 32];
        let pk = ed25519::public_key(&seed);
        let other_pk = ed25519::public_key(&[0x22u8; 32]);
        let pkg = sample_pkg();
        let blob = sign(&pkg, &seed);

        let opened = open(&blob, &[pk]).unwrap();
        assert_eq!(opened.manifest, pkg.manifest);

        // 1. Magic / length -> Malformed even if key wouldn't be trusted
        assert!(matches!(
            open(&blob[..105], &[]),
            Err(PkgError::Malformed(_))
        ));
        let mut bad_magic = blob.clone();
        bad_magic[0] = b'X';
        assert!(matches!(open(&bad_magic, &[]), Err(PkgError::Malformed(_))));

        // 2. Untrusted key -> UntrustedKey even if signature is also corrupted
        let mut bad_sig_and_untrusted = blob.clone();
        bad_sig_and_untrusted[50] ^= 0xff;
        assert_eq!(
            open(&bad_sig_and_untrusted, &[other_pk]),
            Err(PkgError::UntrustedKey)
        );

        // 3. Trusted key, bad signature -> BadSignature
        assert_eq!(
            open(&bad_sig_and_untrusted, &[pk]),
            Err(PkgError::BadSignature)
        );
    }

    #[test]
    fn path_validation_rules() {
        assert!(validate_path("a").is_ok());
        assert!(validate_path("bin/cairn-app").is_ok());
        assert!(validate_path(&"x".repeat(255)).is_ok());

        for bad in [
            "",
            &"x".repeat(256),
            "/abs",
            "trailing/",
            "a//b",
            ".",
            "..",
            "a/./b",
            "a/../b",
            "a\\b",
            "C:foo",
            "a\0b",
            "a\nb",
            "a\x7fb",
        ] {
            assert_eq!(
                validate_path(bad),
                Err(PkgError::BadPath(bad.to_string())),
                "expected BadPath for {bad:?}"
            );
        }
    }

    #[test]
    fn decode_rejects_non_canonical_and_trailing() {
        let pkg = sample_pkg();
        let mut body = encode(&pkg);
        body.push(0);
        assert!(matches!(decode(&body), Err(PkgError::Malformed(_))));

        // Non-canonical manifest (extra comment)
        let raw_manifest = format!("# comment\n{}", pkg.manifest);
        let mut bad = Vec::new();
        bad.extend_from_slice(BODY_MAGIC);
        bad.extend_from_slice(&(raw_manifest.len() as u32).to_le_bytes());
        bad.extend_from_slice(raw_manifest.as_bytes());
        bad.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(decode(&bad), Err(PkgError::Malformed(_))));
    }
}
