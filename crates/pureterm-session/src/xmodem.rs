//! XModem / YModem file transfer protocol support.

/// XModem packet size.
const XMODEM_PACKET_SIZE: usize = 128;
const XMODEM_1K_PACKET_SIZE: usize = 1024;

/// Protocol variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTransferProtocol {
    XModem,
    XModemCrc,
    YModem,
    YModemG,
}

/// XModem control bytes.
#[allow(dead_code)]
mod control {
    pub const SOH: u8 = 0x01;
    pub const STX: u8 = 0x02;
    pub const EOT: u8 = 0x04;
    pub const ACK: u8 = 0x06;
    pub const NAK: u8 = 0x15;
    pub const CAN: u8 = 0x18;
    pub const CRC16: u8 = 0x43; // 'C'
}

/// State machine for XModem/YModem transfers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// Waiting for the receiver to send NAK or 'C'.
    Initial,
    /// Sending/receiving packets.
    Transferring,
    /// Transfer complete.
    Complete,
    /// Transfer cancelled.
    Cancelled,
    /// Transfer error.
    Error,
}

/// Progress of a file transfer.
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub protocol: FileTransferProtocol,
    pub state: TransferState,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
    pub packet_number: u32,
    pub errors: u32,
}

impl TransferProgress {
    pub fn new(protocol: FileTransferProtocol) -> Self {
        Self {
            protocol,
            state: TransferState::Initial,
            bytes_transferred: 0,
            total_bytes: None,
            packet_number: 0,
            errors: 0,
        }
    }

    pub fn percent(&self) -> Option<f64> {
        self.total_bytes.map(|total| {
            if total == 0 { 0.0 } else { (self.bytes_transferred as f64 / total as f64) * 100.0 }
        })
    }
}

/// Calculates CRC-16/XMODEM.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Build an XModem packet (128 bytes + header + checksum).
pub fn build_xmodem_packet(packet_num: u8, data: &[u8; XMODEM_PACKET_SIZE], use_crc: bool) -> Vec<u8> {
    let mut packet = Vec::with_capacity(XMODEM_PACKET_SIZE + 4);
    packet.push(control::SOH);
    packet.push(packet_num);
    packet.push(packet_num ^ 0xFF);
    packet.extend_from_slice(data);
    if use_crc {
        let crc = crc16(data);
        packet.push((crc >> 8) as u8);
        packet.push((crc & 0xFF) as u8);
    } else {
        let checksum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        packet.push(checksum);
    }
    packet
}

/// Detect if data contains XModem/YModem protocol markers.
pub fn detect_transfer_start(data: &[u8]) -> Option<FileTransferProtocol> {
    // XModem is initiated by the receiver sending NAK (0x15) or 'C' (0x43)
    for &byte in data {
        match byte {
            control::NAK => return Some(FileTransferProtocol::XModem),
            control::CRC16 => return Some(FileTransferProtocol::XModemCrc),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc16() {
        // Known CRC-16/XMODEM test vector
        let data = b"123456789";
        let crc = crc16(data);
        assert_eq!(crc, 0x31C3);
    }

    #[test]
    fn test_build_packet() {
        let data = [0u8; XMODEM_PACKET_SIZE];
        let packet = build_xmodem_packet(1, &data, false);
        assert_eq!(packet.len(), XMODEM_PACKET_SIZE + 4); // SOH + num + ~num + data + checksum
        assert_eq!(packet[0], control::SOH);
        assert_eq!(packet[1], 1);
        assert_eq!(packet[2], 0xFE);
    }

    #[test]
    fn test_progress_percent() {
        let mut progress = TransferProgress::new(FileTransferProtocol::XModem);
        assert!(progress.percent().is_none());
        progress.total_bytes = Some(1000);
        progress.bytes_transferred = 500;
        assert_eq!(progress.percent(), Some(50.0));
    }
}
