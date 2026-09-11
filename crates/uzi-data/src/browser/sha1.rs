//! SHA-1 (RFC 3174), required for the RFC 6455 WebSocket handshake.
//!
//! The workspace already depends on `sha2`, but the handshake specifically needs
//! SHA-1, and adding a whole crate for one function is not worth it. This is the
//! textbook implementation — `round 2` of SHA-1 is only used for the
//! `Sec-WebSocket-Accept` check, never for anything security-bearing.

/// SHA-1 digest of `data`.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];

    // Padding: 0x80, zeros, then the 64-bit big-endian bit length.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 80];
    for chunk in msg.chunks_exact(64) {
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: [u8; 20]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// FIPS 180-1 / RFC 3174 published vectors.
    #[test]
    fn matches_published_vectors() {
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(sha1(b"The quick brown fox jumps over the lazy dog")),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    /// One million 'a' exercises multi-block hashing and the length field.
    #[test]
    fn matches_the_million_a_vector() {
        let data = vec![b'a'; 1_000_000];
        assert_eq!(hex(sha1(&data)), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    /// The exact string RFC 6455 §1.3 uses for its handshake example.
    #[test]
    fn matches_the_rfc6455_handshake_example() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        assert_eq!(hex(sha1(key.as_bytes())), "b37a4f2cc0624f1690f64606cf385945b2bec4ea");
    }

    /// Lengths around the 55/56/64-byte padding boundaries must not misbehave.
    #[test]
    fn handles_padding_boundaries() {
        for len in [54usize, 55, 56, 57, 63, 64, 65, 119, 120] {
            let data = vec![b'x'; len];
            let d = sha1(&data);
            // Re-hash to confirm determinism and that no length panics.
            assert_eq!(d, sha1(&data), "len {len}");
        }
    }
}
