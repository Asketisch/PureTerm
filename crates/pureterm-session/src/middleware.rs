use bytes::Bytes;

/// 在终端和会话之间处理数据的中间件。
pub trait SessionMiddleware: Send {
    /// 处理从会话发送到终端的数据。
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes;

    /// 处理从终端发送到会话的数据。
    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes;
}

/// 中间件处理器栈。
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

    /// 通过所有中间件处理从会话到终端的数据。
    pub fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        let mut current = Bytes::copy_from_slice(data);
        for mw in &mut self.middlewares {
            current = mw.process_output_to_terminal(&current);
        }
        current
    }

    /// 通过所有中间件处理从终端到会话的数据。
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
