use bytes::Bytes;

use crate::middleware::SessionMiddleware;

/// Middleware that sends login scripts once when the session first receives output
/// from the terminal (i.e., the user types something for the first time).
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
        // Data from session to terminal passes through unchanged.
        Bytes::copy_from_slice(data)
    }

    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes {
        if self.sent {
            // Already sent login scripts; pass through unchanged.
            return Bytes::copy_from_slice(data);
        }

        self.sent = true;

        // Build the combined output: each script followed by "\r\n", then the
        // original user data.
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
