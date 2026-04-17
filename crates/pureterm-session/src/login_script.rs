use bytes::Bytes;

use crate::middleware::SessionMiddleware;

/// 中间件,在会话首次从终端接收输出时(即用户首次输入内容时)发送一次登录脚本。
pub struct LoginScriptMiddleware {
    scripts: Vec<String>,
    sent: bool,
}

impl LoginScriptMiddleware {
    pub fn new(scripts: Vec<String>) -> Self {
        Self {
            scripts,
            sent: false,
        }
    }
}

impl SessionMiddleware for LoginScriptMiddleware {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes {
        // 从会话到终端的数据原样传递。
        Bytes::copy_from_slice(data)
    }

    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        if self.sent {
            // 已发送登录脚本;原样传递。
            return Bytes::copy_from_slice(data);
        }

        self.sent = true;

        // 构建组合输出:每个脚本后跟 "\r\n",然后是原始用户数据。
        let mut out = Vec::new();
        for script in &self.scripts {
            out.extend_from_slice(script.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(data);
        Bytes::from(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_to_terminal() {
        let mut mw = LoginScriptMiddleware::new(vec!["source ~/.bashrc".into()]);
        let data = b"hello";
        let result = mw.process_output_to_terminal(data);
        assert_eq!(&result[..], b"hello");
    }

    #[test]
    fn prepends_scripts_on_first_call() {
        let mut mw = LoginScriptMiddleware::new(vec![
            "export FOO=bar".into(),
            "source ~/.bashrc".into(),
        ]);
        let result = mw.process_output_to_session(b"ls");
        let expected = b"export FOO=bar\r\nsource ~/.bashrc\r\nls";
        assert_eq!(&result[..], expected);
    }

    #[test]
    fn only_prepends_once() {
        let mut mw = LoginScriptMiddleware::new(vec!["echo hi".into()]);
        let _first = mw.process_output_to_session(b"cmd1");
        let second = mw.process_output_to_session(b"cmd2");
        assert_eq!(&second[..], b"cmd2");
    }
}
