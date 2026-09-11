//! Base64 (RFC 4648 §4), required for the WebSocket handshake key and accept
//! digest.
//!
//! Same reasoning as [`super::sha1`]: one small function, no new dependency.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding.
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18) as usize & 0x3F] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3F] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(triple >> 6) as usize & 0x3F] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[triple as usize & 0x3F] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10 test vectors.
    #[test]
    fn matches_rfc4648_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    /// Padding must be applied for each remainder length.
    #[test]
    fn padding_tracks_the_remainder() {
        for len in 0..12usize {
            let data = vec![b'A'; len];
            let encoded = encode(&data);
            assert_eq!(encoded.len() % 4, 0, "len {len} → {encoded}");
            let pad = encoded.chars().rev().take_while(|c| *c == '=').count();
            assert_eq!(pad, (3 - len % 3) % 3, "len {len} → {encoded}");
        }
    }

    /// High bytes must not be sign-extended or truncated.
    #[test]
    fn handles_all_byte_values() {
        let all: Vec<u8> = (0..=255u8).collect();
        let encoded = encode(&all);
        assert!(encoded.is_ascii());
        // 256 bytes = 85 full triples (255) + 1 trailing byte, so the output is
        // 86 groups ending in "==".
        assert_eq!(encoded.len(), 344);
        assert!(encoded.ends_with("=="), "{encoded}");
        assert_eq!(encoded.chars().filter(|c| *c == '=').count(), 2);
    }

    /// The accept digest from RFC 6455 §1.3's worked example.
    #[test]
    fn produces_the_rfc6455_accept_value() {
        use crate::browser::sha1::sha1;
        const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = encode(&sha1(format!("{key}{GUID}").as_bytes()));
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
