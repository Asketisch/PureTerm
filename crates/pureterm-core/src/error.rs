use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Profile not found: {0}")]
    ProfileNotFound(String),

    #[error("Theme not found: {0}")]
    ThemeNotFound(String),

    #[error("Hotkey conflict: {0}")]
    HotkeyConflict(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::de::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl From<crate::config::ConfigError> for CoreError {
    fn from(e: crate::config::ConfigError) -> Self {
        CoreError::Config(e.to_string())
    }
}
