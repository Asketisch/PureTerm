//! GAP-05:语音输入——按住说话的转写 MVP(Whisper API 留待后续)。

/// `[voice_input]` 配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceInputConfig {
    pub enabled: bool,
    /// OpenAI Whisper 或兼容端点(空 = 禁用)。
    pub api_url: String,
    pub push_to_talk: bool,
}

impl Default for VoiceInputConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: String::new(),
            push_to_talk: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VoiceInputState {
    pub recording: bool,
    pub last_transcript: Option<String>,
}

impl VoiceInputState {
    pub fn toggle_recording(&mut self, cfg: &VoiceInputConfig) -> Result<(), String> {
        if !cfg.enabled {
            return Err("voice_input disabled".into());
        }
        if cfg.api_url.is_empty() {
            return Err("voice_input.api_url not configured".into());
        }
        self.recording = !self.recording;
        if !self.recording {
            // MVP:先返回占位转写文本,待 cpal + Whisper 接入。
            self.last_transcript = Some(String::from("(voice transcription stub)"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_api_url() {
        let mut st = VoiceInputState::default();
        let cfg = VoiceInputConfig {
            enabled: true,
            ..Default::default()
        };
        assert!(st.toggle_recording(&cfg).is_err());
    }
}
