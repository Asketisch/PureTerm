//! Configuration service
//!
//! Provides runtime config loading, saving, watching, and access.

use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::mpsc;

use crate::error::{CoreError, Result};
use crate::theme::ThemeService;

use super::cli::CliArgs;
use super::legacy::{self, AppConfig};

/// Main configuration service managing runtime config state.
pub struct ConfigService {
    #[allow(dead_code)]
    config_dir: PathBuf,
    config: RwLock<AppConfig>,
    theme_service: RwLock<ThemeService>,
    reload_tx: Option<mpsc::Sender<()>>,
}

impl ConfigService {
    pub fn load() -> Result<Self> {
        let config_dir = Self::config_dir();
        std::fs::create_dir_all(&config_dir)?;

        let config = legacy::load_app_config()?;

        let mut theme_service = ThemeService::new();
        theme_service.load_user_themes(&Self::themes_dir());

        Ok(Self {
            config_dir,
            config: RwLock::new(config),
            theme_service: RwLock::new(theme_service),
            reload_tx: None,
        })
    }

    pub fn load_with_cli(_cli: CliArgs) -> Result<Self> {
        // CLI args can be applied on top of loaded config in the future
        Self::load()
    }

    pub fn start_watching(&mut self) -> Result<mpsc::Receiver<()>> {
        use notify::{RecommendedWatcher, RecursiveMode, Watcher};
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        let tx_clone = tx.clone();
        let config_path = Self::config_path();

        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() {
                        let _ = tx_clone.send(());
                    }
                }
            },
            notify::Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| CoreError::Config(e.to_string()))?;

        watcher
            .watch(&config_path, RecursiveMode::NonRecursive)
            .map_err(|e| CoreError::Config(e.to_string()))?;

        self.reload_tx = Some(tx);
        Ok(rx)
    }

    pub fn reload(&self) -> Result<()> {
        let new_config = legacy::load_app_config()?;
        let mut config = self.config.write();
        *config = new_config;
        log::info!("Config reloaded from disk");
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let config = self.config.read();
        legacy::save_app_config(&config)
    }

    pub fn get(&self) -> parking_lot::RwLockReadGuard<'_, AppConfig> {
        self.config.read()
    }

    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut config = self.config.write();
        f(&mut config);
    }

    pub fn theme_service(&self) -> parking_lot::RwLockReadGuard<'_, ThemeService> {
        self.theme_service.read()
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn config_dir() -> PathBuf {
        AppConfig::config_dir()
    }

    pub fn themes_dir() -> PathBuf {
        AppConfig::themes_dir()
    }

    pub fn plugins_dir() -> PathBuf {
        Self::config_dir().join("plugins")
    }
}
