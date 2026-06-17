//! Data-plane framing codec.
//!
//! The daemon↔GUI data plane is a long-lived Unix stream carrying
//! length-prefixed tagged frames:
//!
//! ```text
//! [ u32 LE length ][ u8 type ][ payload .. ]
//! ```
//!
//! `length` counts the type byte plus the payload (`1 + payload.len()`), so a
//! reader consumes `length` bytes after the 4-byte header and splits off the
//! type byte. This keeps the read loop simple: read header, read exactly
//! `length` bytes, decode.

/// Frame type discriminants.
pub mod frame_type {
    pub const OUTPUT: u8 = 0x01; // daemon -> GUI, raw PTY bytes
    pub const INPUT: u8 = 0x02; // GUI -> daemon, bytes to write to PTY stdin
    pub const RESIZE: u8 = 0x03; // GUI -> daemon, payload = cols u16 LE + rows u16 LE
    pub const EXIT: u8 = 0x04; // daemon -> GUI, payload = exit code i32 LE (-1 = unknown)
    pub const PING: u8 = 0x05; // keepalive either way, empty
    pub const PONG: u8 = 0x06; // keepalive either way, empty
    pub const SCROLLBACK: u8 = 0x07; // daemon -> GUI, ring-buffer replay (M2)
}

/// Header size in bytes (the `u32 LE length` field).
pub const HEADER_LEN: usize = 4;

/// Encode a frame into a fresh `Vec<u8>`.
pub fn encode(ftype: u8, payload: &[u8]) -> Vec<u8> {
    let total = 1 + payload.len(); // type byte + payload
    let mut out = Vec::with_capacity(HEADER_LEN + total);
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.push(ftype);
    out.extend_from_slice(payload);
    out
}

/// A decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame<'a> {
    pub ftype: u8,
    pub payload: &'a [u8],
}

/// Decode one frame from the front of `buf`.
///
/// Returns `Ok(Some((frame, consumed)))` if a full frame is available, consuming
/// `consumed` bytes. Returns `Ok(None)` if the buffer does not yet contain a
/// complete frame (partial read). Returns `Err` on a malformed length.
pub fn decode(buf: &[u8]) -> Result<Option<(Frame<'_>, usize)>, &'static str> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&buf[..HEADER_LEN]);
    let total = u32::from_le_bytes(len_bytes) as usize;
    if total == 0 {
        return Err("zero-length frame");
    }
    if buf.len() < HEADER_LEN + total {
        return Ok(None); // need more bytes
    }
    let ftype = buf[HEADER_LEN];
    let payload = &buf[HEADER_LEN + 1..HEADER_LEN + total];
    Ok(Some((Frame { ftype, payload }, HEADER_LEN + total)))
}

/// Encode a RESIZE frame payload (cols, rows).
pub fn encode_resize(cols: u16, rows: u16) -> [u8; 4] {
    let mut p = [0u8; 4];
    p[..2].copy_from_slice(&cols.to_le_bytes());
    p[2..].copy_from_slice(&rows.to_le_bytes());
    p
}

/// Decode a RESIZE frame payload.
pub fn decode_resize(payload: &[u8]) -> Result<(u16, u16), &'static str> {
    if payload.len() != 4 {
        return Err("resize payload must be 4 bytes");
    }
    let cols = u16::from_le_bytes([payload[0], payload[1]]);
    let rows = u16::from_le_bytes([payload[2], payload[3]]);
    Ok((cols, rows))
}

/// Encode an EXIT frame payload (exit code; -1 = unknown).
pub fn encode_exit(code: i32) -> [u8; 4] {
    code.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_output_empty() {
        let wire = encode(frame_type::OUTPUT, b"");
        let (frame, consumed) = decode(&wire).unwrap().expect("full frame");
        assert_eq!(frame.ftype, frame_type::OUTPUT);
        assert_eq!(frame.payload, b"");
        assert_eq!(consumed, wire.len());
    }

    #[test]
    fn roundtrip_output_bytes() {
        let payload = b"hello world\r\n";
        let wire = encode(frame_type::OUTPUT, payload);
        let (frame, consumed) = decode(&wire).unwrap().expect("full frame");
        assert_eq!(frame.ftype, frame_type::OUTPUT);
        assert_eq!(frame.payload, payload);
        assert_eq!(consumed, wire.len());
    }

    #[test]
    fn decode_partial_returns_none() {
        let wire = encode(frame_type::OUTPUT, b"some bytes");
        // Truncate the payload: header present, body incomplete.
        let partial = &wire[..HEADER_LEN + 3];
        assert!(decode(partial).unwrap().is_none());
        // Only header bytes.
        assert!(decode(&wire[..HEADER_LEN - 1]).unwrap().is_none());
    }

    #[test]
    fn decode_consumes_one_of_many() {
        let mut buf = encode(frame_type::OUTPUT, b"first");
        buf.extend(encode(frame_type::INPUT, b"second"));
        let (frame1, n1) = decode(&buf).unwrap().expect("first frame");
        assert_eq!(frame1.payload, b"first");
        let (frame2, n2) = decode(&buf[n1..]).unwrap().expect("second frame");
        assert_eq!(frame2.ftype, frame_type::INPUT);
        assert_eq!(frame2.payload, b"second");
        assert_eq!(n1 + n2, buf.len());
    }

    #[test]
    fn resize_roundtrip() {
        let p = encode_resize(80, 24);
        assert_eq!(decode_resize(&p).unwrap(), (80, 24));
        let wire = encode(frame_type::RESIZE, &p);
        let (frame, _) = decode(&wire).unwrap().unwrap();
        assert_eq!(decode_resize(frame.payload).unwrap(), (80, 24));
    }

    #[test]
    fn resize_bad_payload() {
        assert!(decode_resize(&[0; 3]).is_err());
        assert!(decode_resize(&[0; 5]).is_err());
    }

    #[test]
    fn empty_payload_keepalive() {
        let wire = encode(frame_type::PING, b"");
        let (frame, _) = decode(&wire).unwrap().unwrap();
        assert_eq!(frame.ftype, frame_type::PING);
        assert!(frame.payload.is_empty());
    }

    #[test]
    fn length_includes_type_byte() {
        // payload of 5 bytes => total field = 6 (1 type + 5 payload).
        let wire = encode(frame_type::OUTPUT, b"12345");
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&wire[..HEADER_LEN]);
        assert_eq!(u32::from_le_bytes(len_bytes), 6);
    }
}
