use bytes::Bytes;
use std::sync::{Arc, Mutex};

use crate::middleware::SessionMiddleware;

type CallbackEmitter = Arc<Mutex<Option<Box<dyn FnMut(String) + Send>>>>;

/// Middleware that processes OSC sequences, particularly OSC 7 (current directory).
pub struct OscProcessorMiddleware {
    cwd_reported: CallbackEmitter,
    buffer: Vec<u8>,
    in_osc: bool,
}

impl OscProcessorMiddleware {
    pub fn new() -> Self {
        Self {
            cwd_reported: Arc::new(Mutex::new(None)),
            buffer: Vec::new(),
            in_osc: false,
        }
    }

    pub fn on_cwd<F: FnMut(String) + Send + 'static>(&mut self, f: F) {
        *self.cwd_reported.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
    }
}

impl Default for OscProcessorMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMiddleware for OscProcessorMiddleware {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        // Pass through - OSC processing is handled by the VTE parser
        Bytes::copy_from_slice(data)
    }

    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        // Pass through input
        Bytes::copy_from_slice(data)
    }
}
