use bytes::Bytes;

/// Middleware that processes data between the terminal and session.
pub trait SessionMiddleware: Send {
    /// Process data coming from the session to the terminal.
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes;

    /// Process data coming from the terminal to the session.
    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes;
}

/// Stack of middleware processors.
pub struct MiddlewareStack {
    middlewares: Vec<Box<dyn SessionMiddleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn push(&mut self, mw: Box<dyn SessionMiddleware>) {
        self.middlewares.push(mw);
    }

    /// Process data from session to terminal through all middleware.
    pub fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        let mut current = Bytes::copy_from_slice(data);
        for mw in &mut self.middlewares {
            current = mw.process_output_to_terminal(&current);
        }
        current
    }

    /// Process data from terminal to session through all middleware.
    pub fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        let mut current = Bytes::copy_from_slice(data);
        for mw in &mut self.middlewares {
            current = mw.process_output_to_session(&current);
        }
        current
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}
