//! Telnet IAC (Interpret As Command) protocol negotiation.
//!
//! Handles parsing of Telnet IAC sequences from the data stream, responding
//! to option negotiations, sub-negotiations (SB/SE), and providing clean
//! data bytes stripped of all IAC commands.

// ── IAC command bytes ────────────────────────────────────────────────
const IAC: u8 = 255;
const WILL: u8 = 251;
const WONT: u8 = 252;
const DO: u8 = 253;
const DONT: u8 = 254;
const SB: u8 = 250; // Subnegotiation Begin
const SE: u8 = 240; // Subnegotiation End

// ── Well-known Telnet options ────────────────────────────────────────
const OPT_ECHO: u8 = 1;
const OPT_SUPPRESS_GA: u8 = 3;
const OPT_TERMINAL_TYPE: u8 = 24;
const OPT_NAWS: u8 = 31; // Negotiate About Window Size
const OPT_TERMINAL_SPEED: u8 = 32;

/// Terminal type string advertised to remote hosts.
const TERMINAL_TYPE: &[u8] = b"xterm-256color";

/// Parser state used inside `process_input` to track where we are in an
/// IAC sequence across calls.
#[derive(Debug, Clone, PartialEq)]
enum IacState {
    /// Normal data processing.
    Data,
    /// Saw IAC byte, waiting for the command byte.
    Iac,
    /// Saw IAC + WILL, waiting for the option byte.
    Will,
    /// Saw IAC + WONT, waiting for the option byte.
    Wont,
    /// Saw IAC + DO, waiting for the option byte.
    Do,
    /// Saw IAC + DONT, waiting for the option byte.
    Dont,
    /// Inside a subnegotiation (IAC SB received).
    Subneg {
        /// The option being sub-negotiated.
        option: u8,
        /// Collected subnegotiation data (between SB and IAC SE).
        data: Vec<u8>,
    },
    /// Inside a subnegotiation and the previous byte was IAC (could be SE
    /// or an escaped 0xFF).
    SubnegIac {
        option: u8,
        data: Vec<u8>,
    },
}

/// Tracks which options we have agreed to with the remote side.
#[derive(Debug, Clone)]
struct NegotiatedOptions {
    /// We sent WILL <opt> and it was accepted (or we just decided to).
    will_naws: bool,
    /// Remote asked for terminal-type sub-negotiation.
    will_terminal_type: bool,
}

/// IAC negotiator that processes raw Telnet bytes and produces clean data
/// along with any responses that should be sent back.
pub struct IacNegotiator {
    state: IacState,
    options: NegotiatedOptions,
    /// Current terminal dimensions (cols, rows). Used for NAWS responses.
    window_size: (u16, u16),
}

impl Default for IacNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

impl IacNegotiator {
    /// Create a new negotiator with default window size (80x24).
    pub fn new() -> Self {
        Self {
            state: IacState::Data,
            options: NegotiatedOptions {
                will_naws: false,
                will_terminal_type: false,
            },
            window_size: (80, 24),
        }
    }

    /// Update the stored window dimensions. Returns IAC NAWS bytes to send
    /// if NAWS was previously negotiated, or an empty vec otherwise.
    pub fn set_window_size(&mut self, cols: u16, rows: u16) -> Vec<u8> {
        self.window_size = (cols, rows);
        if self.options.will_naws {
            self.build_naws_response()
        } else {
            Vec::new()
        }
    }

    /// Process a chunk of raw bytes received from the Telnet server.
    ///
    /// Returns a tuple of `(clean_data, responses_to_send)`.
    ///
    /// - `clean_data`: the original bytes with all IAC sequences removed.
    /// - `responses_to_send`: IAC command bytes that should be written back
    ///   to the server (e.g. DO/DONT/WILL/WONT replies, subnegotiation
    ///   responses).
    pub fn process_input(&mut self, data: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut clean = Vec::new();
        let mut responses = Vec::new();

        for &byte in data {
            match self.state.clone() {
                IacState::Data => {
                    if byte == IAC {
                        self.state = IacState::Iac;
                    } else {
                        clean.push(byte);
                    }
                }

                IacState::Iac => {
                    match byte {
                        IAC => {
                            // Escaped 0xFF – literal byte in data.
                            clean.push(IAC);
                            self.state = IacState::Data;
                        }
                        WILL => {
                            self.state = IacState::Will;
                        }
                        WONT => {
                            self.state = IacState::Wont;
                        }
                        DO => {
                            self.state = IacState::Do;
                        }
                        DONT => {
                            self.state = IacState::Dont;
                        }
                        SB => {
                            // Next byte will be the subnegotiation option.
                            self.state = IacState::Subneg {
                                option: 0,
                                data: Vec::new(),
                            };
                            // We'll set the real option on the next byte.
                            // Use a temporary marker – handled below.
                        }
                        SE => {
                            // Stray SE outside subnegotiation – ignore.
                            self.state = IacState::Data;
                        }
                        // Other two-byte IAC commands (AYT, NOP, etc.): ignore.
                        _ => {
                            self.state = IacState::Data;
                        }
                    }
                }

                IacState::Will => {
                    responses.extend(self.handle_will(byte));
                    self.state = IacState::Data;
                }

                IacState::Wont => {
                    // The server won't do something – acknowledge with DONT.
                    responses.push(IAC);
                    responses.push(DONT);
                    responses.push(byte);
                    self.state = IacState::Data;
                }

                IacState::Do => {
                    responses.extend(self.handle_do(byte));
                    self.state = IacState::Data;
                }

                IacState::Dont => {
                    // Server says don't – we comply with WONT.
                    if byte == OPT_NAWS {
                        self.options.will_naws = false;
                    }
                    if byte == OPT_TERMINAL_TYPE {
                        self.options.will_terminal_type = false;
                    }
                    responses.push(IAC);
                    responses.push(WONT);
                    responses.push(byte);
                    self.state = IacState::Data;
                }

                IacState::Subneg { option, .. } => {
                    if option == 0 {
                        // First byte of subnegotiation is the option code.
                        self.state = IacState::Subneg {
                            option: byte,
                            data: Vec::new(),
                        };
                    } else if byte == IAC {
                        self.state = IacState::SubnegIac {
                            option,
                            data: Vec::new(),
                        };
                    } else {
                        // Accumulate subnegotiation data – we rebuild the vec
                        // each time to avoid borrowing issues.
                        let mut d = Vec::new();
                        d.push(byte);
                        self.state = IacState::Subneg { option, data: d };
                    }
                }

                IacState::SubnegIac { option, data } => {
                    if byte == SE {
                        // End of subnegotiation.
                        responses.extend(self.handle_subneg(option, &data));
                        self.state = IacState::Data;
                    } else if byte == IAC {
                        // Escaped 0xFF inside subneg data.
                        let mut d = data.clone();
                        d.push(IAC);
                        self.state = IacState::Subneg { option, data: d };
                    } else {
                        // Unexpected – treat as data inside subneg.
                        let mut d = data.clone();
                        d.push(IAC);
                        d.push(byte);
                        self.state = IacState::Subneg { option, data: d };
                    }
                }
            }
        }

        (clean, responses)
    }

    // ── Negotiation helpers ──────────────────────────────────────────

    /// Handle `IAC WILL <option>` from the server.
    fn handle_will(&mut self, option: u8) -> Vec<u8> {
        match option {
            OPT_SUPPRESS_GA => {
                // Accept: we want the server to suppress Go-Ahead.
                vec![IAC, DO, option]
            }
            OPT_ECHO => {
                // We don't want remote echo – refuse.
                vec![IAC, DONT, option]
            }
            _ => {
                // Unknown option – refuse.
                vec![IAC, DONT, option]
            }
        }
    }

    /// Handle `IAC DO <option>` from the server (server asks us to enable
    /// an option).
    fn handle_do(&mut self, option: u8) -> Vec<u8> {
        match option {
            OPT_TERMINAL_TYPE => {
                self.options.will_terminal_type = true;
                vec![IAC, WILL, option]
            }
            OPT_NAWS => {
                self.options.will_naws = true;
                let mut resp = vec![IAC, WILL, option];
                resp.extend(self.build_naws_response());
                resp
            }
            OPT_SUPPRESS_GA => {
                vec![IAC, WILL, option]
            }
            OPT_TERMINAL_SPEED => {
                // We don't implement terminal-speed reporting.
                vec![IAC, WONT, option]
            }
            _ => {
                vec![IAC, WONT, option]
            }
        }
    }

    /// Handle a completed subnegotiation for `option` with the given
    /// `sub_data`.
    fn handle_subneg(&mut self, option: u8, sub_data: &[u8]) -> Vec<u8> {
        match option {
            OPT_TERMINAL_TYPE => {
                // RFC 1091: server sends IAC SB TERMINAL-TYPE 1 (SEND) IAC SE
                // We respond with IAC SB TERMINAL-TYPE 0 (IS) <type> IAC SE
                if sub_data.first() == Some(&1) {
                    // SEND request
                    let mut resp = vec![IAC, SB, OPT_TERMINAL_TYPE, 0]; // 0 = IS
                    resp.extend_from_slice(TERMINAL_TYPE);
                    resp.push(IAC);
                    resp.push(SE);
                    resp
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// Build a NAWS (Negotiate About Window Size) subnegotiation message
    /// with the current window dimensions.
    ///
    /// Format per RFC 1073:
    /// `IAC SB NAWS <width-hi> <width-lo> <height-hi> <height-lo> IAC SE`
    ///
    /// Each 16-bit value is sent in network byte order (big-endian). Any
    /// 0xFF bytes must be escaped as `IAC IAC` per Telnet convention.
    fn build_naws_response(&self) -> Vec<u8> {
        let (cols, rows) = self.window_size;
        let mut resp = vec![IAC, SB, OPT_NAWS];
        resp.extend(escape_u16_be(cols));
        resp.extend(escape_u16_be(rows));
        resp.push(IAC);
        resp.push(SE);
        resp
    }
}

/// Encode a u16 as two big-endian bytes, escaping any 0xFF values per
/// Telnet rules (a literal 0xFF in a subnegotiation must be sent as
/// `IAC IAC`, i.e. `255 255`).
fn escape_u16_be(value: u16) -> Vec<u8> {
    let hi = (value >> 8) as u8;
    let lo = (value & 0xFF) as u8;
    let mut out = Vec::with_capacity(4);
    if hi == IAC {
        out.push(IAC);
        out.push(IAC);
    } else {
        out.push(hi);
    }
    if lo == IAC {
        out.push(IAC);
        out.push(IAC);
    } else {
        out.push(lo);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_data_passes_through() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(b"hello world");
        assert_eq!(data, b"hello world");
        assert!(resp.is_empty());
    }

    #[test]
    fn test_escaped_0xff_becomes_literal() {
        let mut neg = IacNegotiator::new();
        let (data, _resp) = neg.process_input(&[IAC, IAC]);
        assert_eq!(data, vec![IAC]);
    }

    #[test]
    fn test_will_suppress_ga_responds_do() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, WILL, OPT_SUPPRESS_GA]);
        assert!(data.is_empty());
        assert_eq!(resp, vec![IAC, DO, OPT_SUPPRESS_GA]);
    }

    #[test]
    fn test_will_echo_responds_dont() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, WILL, OPT_ECHO]);
        assert!(data.is_empty());
        assert_eq!(resp, vec![IAC, DONT, OPT_ECHO]);
    }

    #[test]
    fn test_will_unknown_responds_dont() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, WILL, 99]);
        assert!(data.is_empty());
        assert_eq!(resp, vec![IAC, DONT, 99]);
    }

    #[test]
    fn test_do_terminal_type_responds_will() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, DO, OPT_TERMINAL_TYPE]);
        assert!(data.is_empty());
        assert_eq!(resp, vec![IAC, WILL, OPT_TERMINAL_TYPE]);
        assert!(neg.options.will_terminal_type);
    }

    #[test]
    fn test_do_naws_responds_will_plus_naws() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, DO, OPT_NAWS]);
        assert!(data.is_empty());
        // Should start with IAC WILL NAWS, then NAWS subneg.
        assert!(resp.starts_with(&[IAC, WILL, OPT_NAWS]));
        assert!(neg.options.will_naws);
        // Verify NAWS subneg is present after WILL.
        let naws_start = resp
            .windows(3)
            .position(|w| w == [IAC, SB, OPT_NAWS])
            .expect("NAWS SB should be present");
        let naws_msg = &resp[naws_start..];
        // IAC SB NAWS <4 bytes dims> IAC SE
        assert_eq!(naws_msg[naws_msg.len() - 2], IAC);
        assert_eq!(naws_msg[naws_msg.len() - 1], SE);
    }

    #[test]
    fn test_do_unknown_responds_wont() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, DO, 99]);
        assert!(data.is_empty());
        assert_eq!(resp, vec![IAC, WONT, 99]);
    }

    #[test]
    fn test_dont_naws_clears_flag() {
        let mut neg = IacNegotiator::new();
        // First negotiate NAWS on.
        let (_, resp) = neg.process_input(&[IAC, DO, OPT_NAWS]);
        assert!(!resp.is_empty());
        assert!(neg.options.will_naws);
        // Now server sends DONT NAWS.
        let (_, resp2) = neg.process_input(&[IAC, DONT, OPT_NAWS]);
        assert_eq!(resp2, vec![IAC, WONT, OPT_NAWS]);
        assert!(!neg.options.will_naws);
    }

    #[test]
    fn test_subneg_terminal_type_send() {
        let mut neg = IacNegotiator::new();
        // First agree to terminal type.
        let (_, _) = neg.process_input(&[IAC, DO, OPT_TERMINAL_TYPE]);
        // Server sends: IAC SB TERMINAL-TYPE 1(SEND) IAC SE
        let input: Vec<u8> = vec![IAC, SB, OPT_TERMINAL_TYPE, 1, IAC, SE];
        let (data, resp) = neg.process_input(&input);
        assert!(data.is_empty());
        // Expected: IAC SB TERMINAL-TYPE 0(IS) "xterm-256color" IAC SE
        let expected: Vec<u8> = {
            let mut v = vec![IAC, SB, OPT_TERMINAL_TYPE, 0];
            v.extend_from_slice(TERMINAL_TYPE);
            v.push(IAC);
            v.push(SE);
            v
        };
        assert_eq!(resp, expected);
    }

    #[test]
    fn test_mixed_data_and_iac() {
        let mut neg = IacNegotiator::new();
        let input: Vec<u8> = [
            b'h', b'e', b'l', b'l', b'o', // data
            IAC, WILL, OPT_SUPPRESS_GA, // command
            b' ', b'w', b'o', b'r', b'l', b'd', // data
        ]
        .to_vec();
        let (data, resp) = neg.process_input(&input);
        assert_eq!(data, b"hello world");
        assert_eq!(resp, vec![IAC, DO, OPT_SUPPRESS_GA]);
    }

    #[test]
    fn test_set_window_size_after_naws_negotiated() {
        let mut neg = IacNegotiator::new();
        // Negotiate NAWS first.
        let _ = neg.process_input(&[IAC, DO, OPT_NAWS]);
        // Now resize.
        let resp = neg.set_window_size(120, 40);
        // Should contain NAWS subneg with 120 x 40.
        assert!(!resp.is_empty());
        let sb_start = resp
            .windows(3)
            .position(|w| w == [IAC, SB, OPT_NAWS])
            .unwrap();
        // Bytes after IAC SB NAWS: width-hi, width-lo, height-hi, height-lo
        assert_eq!(resp[sb_start + 3], 0); // 120 >> 8
        assert_eq!(resp[sb_start + 4], 120); // 120 & 0xFF
        assert_eq!(resp[sb_start + 5], 0); // 40 >> 8
        assert_eq!(resp[sb_start + 6], 40); // 40 & 0xFF
    }

    #[test]
    fn test_set_window_size_before_naws_returns_empty() {
        let mut neg = IacNegotiator::new();
        let resp = neg.set_window_size(120, 40);
        assert!(resp.is_empty());
    }

    #[test]
    fn test_naws_with_ff_in_dimension() {
        let mut neg = IacNegotiator::new();
        neg.options.will_naws = true;
        let resp = neg.set_window_size(0xFF01, 0x01FF);
        let sb_start = resp
            .windows(3)
            .position(|w| w == [IAC, SB, OPT_NAWS])
            .unwrap();
        // width hi = 0xFF → escaped as IAC IAC
        assert_eq!(resp[sb_start + 3], IAC);
        assert_eq!(resp[sb_start + 4], IAC);
        assert_eq!(resp[sb_start + 5], 0x01);
        // height lo = 0xFF → escaped as IAC IAC
        assert_eq!(resp[sb_start + 6], 0x01);
        assert_eq!(resp[sb_start + 7], IAC);
        assert_eq!(resp[sb_start + 8], IAC);
    }

    #[test]
    fn test_iac_sequence_split_across_calls() {
        let mut neg = IacNegotiator::new();
        // First chunk: data + IAC (incomplete).
        let (data1, resp1) = neg.process_input(&[b'A', IAC]);
        assert_eq!(data1, b"A");
        assert!(resp1.is_empty());
        // Second chunk: WILL SUPPRESS_GA + more data.
        let (data2, resp2) = neg.process_input(&[WILL, OPT_SUPPRESS_GA, b'B']);
        assert!(data2.is_empty() || data2 == b"B");
        // The WILL is consumed, 'B' is clean data.
        assert_eq!(data2, b"B");
        assert_eq!(resp2, vec![IAC, DO, OPT_SUPPRESS_GA]);
    }

    #[test]
    fn test_wont_from_server_responds_dont() {
        let mut neg = IacNegotiator::new();
        let (data, resp) = neg.process_input(&[IAC, WONT, OPT_ECHO]);
        assert!(data.is_empty());
        assert_eq!(resp, vec![IAC, DONT, OPT_ECHO]);
    }
}
