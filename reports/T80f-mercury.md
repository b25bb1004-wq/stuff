# T80f mercury fix (request smuggling, RFC 9112 6.3)
- Fixed: parse_framing (body.rs) now refuses a message with both Transfer-Encoding: chunked and Content-Length (new Error::AmbiguousFraming); used by parse_request and parse_response.
- Content-Length is always validated now, so differing duplicate values give BadContentLength on every path. Transfer-Encoding rule (final coding must be chunked, no duplicates) was already enforced and is unchanged.
- Flipped three existing tests that pinned the defect: corpus.rs case 13 and response case 9, leader_http.rs smuggling block. Added 3 tests in security.rs.
- Gate: `CHECK PASS mercury` (fmt, clippy -D warnings, all tests, zero-dependency). Nothing committed; no other crate touched by this work.
