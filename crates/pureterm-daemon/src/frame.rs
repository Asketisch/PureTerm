//! 数据面帧编解码器。
//!
//! 守护进程↔GUI 数据面是一条承载长度前缀带标签帧的
//! 长连 Unix 流:
//!
//! ```text
//! [ u32 LE length ][ u8 type ][ payload .. ]
//! ```
//!
//! `length` 计入类型字节加载荷(`1 + payload.len()`),因此
//! 读取方在 4 字节头之后消费 `length` 字节并拆出
//! 类型字节。这使读循环很简单:读头、精确读
//! `length` 字节、解码。

/// 帧类型判别值。
pub mod frame_type {
    pub const OUTPUT: u8 = 0x01; // daemon -> GUI, raw PTY bytes
    pub const INPUT: u8 = 0x02; // GUI -> daemon, bytes to write to PTY stdin
    pub const RESIZE: u8 = 0x03; // GUI -> daemon, payload = cols u16 LE + rows u16 LE
    pub const EXIT: u8 = 0x04; // daemon -> GUI, payload = exit code i32 LE (-1 = unknown)
    pub const PING: u8 = 0x05; // keepalive either way, empty
    pub const PONG: u8 = 0x06; // keepalive either way, empty
    pub const SCROLLBACK: u8 = 0x07; // daemon -> GUI, ring-buffer replay (M2)
}

/// 头部大小(字节)(即 `u32 LE length` 字段)。
pub const HEADER_LEN: usize = 4;

/// 把一帧编码进新的 `Vec<u8>`。
pub fn encode(ftype: u8, payload: &[u8]) -> Vec<u8> {
    let total = 1 + payload.len(); // type byte + payload
    let mut out = Vec::with_capacity(HEADER_LEN + total);
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.push(ftype);
    out.extend_from_slice(payload);
    out
}

/// 一个已解码的帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame<'a> {
    pub ftype: u8,
    pub payload: &'a [u8],
}

/// 从 `buf` 前部解码一帧。
///
/// 完整帧可用时返回 `Ok(Some((frame, consumed)))`,消费
/// `consumed` 字节。缓冲尚不含完整帧(部分读)时返回
/// `Ok(None)`。长度畸形返回 `Err`。
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

/// 编码 RESIZE 帧载荷(cols、rows)。
pub fn encode_resize(cols: u16, rows: u16) -> [u8; 4] {
    let mut p = [0u8; 4];
    p[..2].copy_from_slice(&cols.to_le_bytes());
    p[2..].copy_from_slice(&rows.to_le_bytes());
    p
}

/// 解码 RESIZE 帧载荷。
pub fn decode_resize(payload: &[u8]) -> Result<(u16, u16), &'static str> {
    if payload.len() != 4 {
        return Err("resize payload must be 4 bytes");
    }
    let cols = u16::from_le_bytes([payload[0], payload[1]]);
    let rows = u16::from_le_bytes([payload[2], payload[3]]);
    Ok((cols, rows))
}

/// 编码 EXIT 帧载荷(退出码;-1 = 未知)。
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
        // 截断载荷:有头,体不完整。
        let partial = &wire[..HEADER_LEN + 3];
        assert!(decode(partial).unwrap().is_none());
        // 只有头部字节。
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
        // 载荷 5 字节 => total 字段 = 6(1 类型 + 5 载荷)。
        let wire = encode(frame_type::OUTPUT, b"12345");
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&wire[..HEADER_LEN]);
        assert_eq!(u32::from_le_bytes(len_bytes), 6);
    }
}
