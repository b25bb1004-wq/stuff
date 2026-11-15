//! TCP network server, client, and binary row codec for Iridium.

mod client;
mod codec;
mod server;

pub use client::Client;
pub use codec::{decode_rows, encode_rows};
pub use server::Server;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{decode_rows, encode_rows};
    use crate::sql::{Rows, Value};

    #[test]
    fn codec_round_trip_all_types() {
        let rows = Rows {
            columns: vec!["n".into(), "i".into(), "t".into(), "b".into()],
            rows: vec![
                vec![
                    Value::Null,
                    Value::Int(-42),
                    Value::Text("hello\tvarve".into()),
                    Value::Blob(vec![0x00, 0xff, 0x10]),
                ],
                vec![
                    Value::Null,
                    Value::Int(i64::MAX),
                    Value::Text(String::new()),
                    Value::Blob(Vec::new()),
                ],
            ],
            changes: 2,
        };
        let encoded = encode_rows(&rows);
        assert_eq!(decode_rows(&encoded), Some(rows));
    }

    #[test]
    fn codec_rejects_malformed_inputs() {
        let rows = Rows {
            columns: vec!["a".into()],
            rows: vec![vec![Value::Int(1)]],
            changes: 1,
        };
        let mut encoded = encode_rows(&rows);
        // Trailing byte
        encoded.push(0x00);
        assert_eq!(decode_rows(&encoded), None);
        encoded.pop();
        // Truncated
        for cut in 0..encoded.len() {
            assert_eq!(decode_rows(&encoded[..cut]), None);
        }
        // Invalid tag
        let mut bad_tag = encoded.clone();
        bad_tag[17] = 9;
        assert_eq!(decode_rows(&bad_tag), None);
        // Wrong width: 0 columns, 1 row
        let mut zero_cols_one_row = Vec::new();
        zero_cols_one_row.extend_from_slice(&0u64.to_be_bytes());
        zero_cols_one_row.extend_from_slice(&0u32.to_be_bytes());
        zero_cols_one_row.extend_from_slice(&1u32.to_be_bytes());
        assert_eq!(decode_rows(&zero_cols_one_row), None);
    }
}
