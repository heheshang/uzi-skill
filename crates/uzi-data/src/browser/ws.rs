//! Minimal RFC 6455 WebSocket **client**, enough for the Chrome DevTools
//! Protocol.
//!
//! CDP is JSON over a single WebSocket, so this only implements what a client
//! needs: a text-frame send/receive pair, client-side masking, fragmented reads,
//! and the control frames the server can interleave (`ping` → `pong`, `close`).
//! Compression, subprotocol negotiation, and TLS are not supported — CDP is
//! plain `ws://` on loopback.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::base64;
use super::sha1::sha1;

/// RFC 6455 §1.3 handshake GUID.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

const OP_CONTINUATION: u8 = 0x0;
const OP_TEXT: u8 = 0x1;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

/// Frames larger than this abort the read, so a hostile or broken peer cannot
/// make us allocate without bound. CDP payloads (page HTML) sit far below it.
const MAX_FRAME_BYTES: u64 = 64 * 1024 * 1024;
/// Cap on a reassembled message, matching [`MAX_FRAME_BYTES`].
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum WsError {
    Io(std::io::Error),
    /// The handshake response did not carry `101 Switching Protocols`.
    Handshake(String),
    /// The peer closed the connection.
    Closed,
    Protocol(String),
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::Io(e) => write!(f, "websocket io: {e}"),
            WsError::Handshake(s) => write!(f, "websocket handshake failed: {s}"),
            WsError::Closed => write!(f, "websocket closed"),
            WsError::Protocol(s) => write!(f, "websocket protocol error: {s}"),
        }
    }
}

impl std::error::Error for WsError {}

impl From<std::io::Error> for WsError {
    fn from(e: std::io::Error) -> Self {
        WsError::Io(e)
    }
}

type Result<T, E = WsError> = std::result::Result<T, E>;

/// A connected WebSocket client.
pub struct Ws {
    stream: TcpStream,
}

impl Ws {
    /// Open a `ws://host:port/path` connection.
    pub fn connect(host: &str, port: u16, path: &str, timeout: Duration) -> Result<Ws> {
        let stream = TcpStream::connect((host, port))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;
        Ws::handshake(stream, host, port, path)
    }

    /// Open a connection from a full `ws://` URL, as returned by `/json/version`.
    pub fn connect_ws_url(url: &str, timeout: Duration) -> Result<Ws> {
        let rest = url
            .strip_prefix("ws://")
            .ok_or_else(|| WsError::Protocol(format!("only ws:// is supported, got {url:?}")))?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse::<u16>()
                    .map_err(|_| WsError::Protocol(format!("bad port in {url:?}")))?,
            ),
            None => (authority.to_string(), 80),
        };
        Ws::connect(&host, port, path, timeout)
    }

    fn handshake(
        stream: TcpStream,
        host: &str,
        port: u16,
        path: &str,
    ) -> Result<Ws> {
        let key = handshake_key();
        let request = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n"
        );

        let mut ws = Ws { stream };
        ws.stream.write_all(request.as_bytes())?;
        ws.stream.flush()?;
        ws.read_handshake(&key)?;
        Ok(ws)
    }

    /// Change the read timeout used by [`Ws::recv_text`].
    pub fn set_read_timeout(&self, timeout: Duration) -> Result<()> {
        self.stream.set_read_timeout(Some(timeout))?;
        Ok(())
    }

    /// Read and validate the `101` response, including the accept digest.
    fn read_handshake(&mut self, key: &str) -> Result<()> {
        // Read headers one byte at a time; the response is small and this avoids
        // over-reading into the first frame.
        let mut buf = Vec::with_capacity(512);
        let mut byte = [0u8; 1];
        while !buf.ends_with(b"\r\n\r\n") {
            if buf.len() > 16 * 1024 {
                return Err(WsError::Handshake("response headers too large".into()));
            }
            match self.stream.read_exact(&mut byte) {
                Ok(()) => buf.push(byte[0]),
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                    return Err(WsError::Handshake("connection closed during handshake".into()))
                }
                Err(e) => return Err(WsError::Io(e)),
            }
        }
        let text = String::from_utf8_lossy(&buf).to_string();

        let status = text.lines().next().unwrap_or_default();
        if !status.contains("101") {
            return Err(WsError::Handshake(status.trim().to_string()));
        }

        let expected = base64::encode(&sha1(format!("{key}{WS_GUID}").as_bytes()));
        let got = text
            .lines()
            .find_map(|l| {
                let (name, value) = l.split_once(':')?;
                name.eq_ignore_ascii_case("sec-websocket-accept")
                    .then(|| value.trim().to_string())
            })
            .unwrap_or_default();
        if got != expected {
            return Err(WsError::Handshake(format!(
                "bad Sec-WebSocket-Accept: {got:?} != {expected:?}"
            )));
        }
        Ok(())
    }

    /// Send a text message as a single masked frame.
    pub fn send_text(&mut self, text: &str) -> Result<()> {
        self.write_frame(OP_TEXT, text.as_bytes())
    }

    /// Receive the next text message, transparently handling control frames,
    /// fragmentation, and interleaved pings.
    pub fn recv_text(&mut self) -> Result<String> {
        let mut assembled: Vec<u8> = Vec::new();
        loop {
            let (opcode, fin, payload) = self.read_frame()?;
            match opcode {
                OP_PING => self.write_frame(OP_PONG, &payload)?,
                OP_PONG => {}
                OP_CLOSE => return Err(WsError::Closed),
                OP_TEXT | OP_CONTINUATION => {
                    if opcode == OP_CONTINUATION && assembled.is_empty() {
                        return Err(WsError::Protocol(
                            "continuation frame without a start frame".into(),
                        ));
                    }
                    if assembled.len() + payload.len() > MAX_MESSAGE_BYTES {
                        return Err(WsError::Protocol("message exceeds size cap".into()));
                    }
                    assembled.extend_from_slice(&payload);
                    if fin {
                        return String::from_utf8(assembled)
                            .map_err(|e| WsError::Protocol(format!("invalid utf-8: {e}")));
                    }
                }
                other => {
                    return Err(WsError::Protocol(format!("unexpected opcode {other:#x}")))
                }
            }
        }
    }

    /// Close the connection politely; errors are ignored since the caller is
    /// discarding the socket anyway.
    pub fn close(&mut self) {
        let _ = self.write_frame(OP_CLOSE, &[]);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<()> {
        // FIN + opcode.
        let mut frame = Vec::with_capacity(payload.len() + 14);
        frame.push(0x80 | opcode);

        // Client frames MUST be masked, so the mask bit is always set.
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }

        let mask = masking_key();
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));

        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Read one frame, returning `(opcode, fin, payload)`.
    fn read_frame(&mut self) -> Result<(u8, bool, Vec<u8>)> {
        let mut header = [0u8; 2];
        self.stream.read_exact(&mut header)?;

        let fin = header[0] & 0x80 != 0;
        let opcode = header[0] & 0x0F;
        let masked = header[1] & 0x80 != 0;
        let short_len = (header[1] & 0x7F) as u64;

        let len = match short_len {
            126 => {
                let mut b = [0u8; 2];
                self.stream.read_exact(&mut b)?;
                u16::from_be_bytes(b) as u64
            }
            127 => {
                let mut b = [0u8; 8];
                self.stream.read_exact(&mut b)?;
                u64::from_be_bytes(b)
            }
            n => n,
        };
        if len > MAX_FRAME_BYTES {
            return Err(WsError::Protocol(format!("frame too large: {len} bytes")));
        }

        // A server must not mask; tolerate it by unmasking rather than failing.
        let mask = if masked {
            let mut m = [0u8; 4];
            self.stream.read_exact(&mut m)?;
            Some(m)
        } else {
            None
        };

        let mut payload = vec![0u8; len as usize];
        self.stream.read_exact(&mut payload)?;
        if let Some(m) = mask {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= m[i % 4];
            }
        }
        Ok((opcode, fin, payload))
    }
}

/// 16 random bytes, base64-encoded, as the `Sec-WebSocket-Key`.
///
/// `/dev/urandom` is used where available; the key is a loopback CSRF guard, not
/// a secret, so a clock-seeded fallback is acceptable on platforms without it.
fn handshake_key() -> String {
    let mut bytes = [0u8; 16];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_err()
    {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        let mut state = seed | 1;
        for b in bytes.iter_mut() {
            // xorshift64* — adequate for a non-secret handshake nonce.
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            *b = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u8;
        }
    }
    base64::encode(&bytes)
}

fn masking_key() -> [u8; 4] {
    let mut bytes = [0u8; 4];
    match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)) {
        Ok(()) => bytes,
        Err(_) => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            nanos.to_le_bytes()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_key_is_16_random_bytes_base64_encoded() {
        let a = handshake_key();
        let b = handshake_key();
        assert_eq!(a.len(), 24, "16 bytes → 24 base64 chars with padding: {a}");
        assert!(a.ends_with("=="));
        assert_ne!(a, b, "keys must not repeat");
    }

    #[test]
    fn masking_keys_vary() {
        let keys: std::collections::HashSet<[u8; 4]> = (0..64).map(|_| masking_key()).collect();
        assert!(keys.len() > 1, "masking keys should not be constant");
    }

    /// A frame built by the client must be masked and carry the right length
    /// encoding at each width boundary.
    #[test]
    fn client_frames_are_masked_with_correct_length_encoding() {
        // Build the same bytes `write_frame` would, without a socket.
        let enc = |len: usize| -> Vec<u8> {
            let mut f = vec![0x80 | OP_TEXT];
            if len < 126 {
                f.push(0x80 | len as u8);
            } else if len <= u16::MAX as usize {
                f.push(0x80 | 126);
                f.extend_from_slice(&(len as u16).to_be_bytes());
            } else {
                f.push(0x80 | 127);
                f.extend_from_slice(&(len as u64).to_be_bytes());
            }
            f
        };

        assert_eq!(enc(0), vec![0x81, 0x80]);
        assert_eq!(enc(125), vec![0x81, 0x80 | 125]);
        assert_eq!(enc(126), vec![0x81, 0x80 | 126, 0x00, 0x7E]);
        assert_eq!(enc(300), vec![0x81, 0x80 | 126, 0x01, 0x2C]);
        let big = enc(70_000);
        assert_eq!(big[1], 0x80 | 127);
        assert_eq!(u64::from_be_bytes(big[2..10].try_into().unwrap()), 70_000);
    }

    /// Unmasking is its own inverse, so the read path must recover the payload.
    #[test]
    fn masking_round_trips() {
        let payload = b"hello websocket".repeat(10);
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        let restored: Vec<u8> = masked
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        assert_eq!(restored, payload);
    }

    #[test]
    fn rfc6455_accept_digest_is_correct() {
        // The §1.3 worked example: this is what `read_handshake` validates.
        let accept = base64::encode(&sha1(
            format!("dGhlIHNhbXBsZSBub25jZQ=={WS_GUID}").as_bytes(),
        ));
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }
}
