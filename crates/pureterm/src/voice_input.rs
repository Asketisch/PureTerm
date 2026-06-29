//! GAP-05: Voice input — push-to-talk transcription MVP (Whisper API deferred).

/// `[voice_input]` config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceInputConfig {
    pub enabled: bool,
    /// OpenAI Whisper or compatible endpoint (empty = disabled).
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
            // MVP: stub transcript until cpal + Whisper wired.
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
