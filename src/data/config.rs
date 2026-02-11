//! Application Configuration
//!
//! Handles loading and saving application configuration.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub asr: AsrConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            hotkey: HotkeyConfig::default(),
            asr: AsrConfig::default(),
        }
    }
}

impl AppConfig {
    /// Get the config file path
    pub fn config_path() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        exe_dir.join("config.toml")
    }

    /// Get the credentials file path
    pub fn credentials_path() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        exe_dir.join("credentials.json")
    }

    /// Load configuration from file or create default
    pub fn load_or_default() -> Result<Self> {
        let path = Self::config_path();

        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let config: AppConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = AppConfig::default();
            config.save()?;
            Ok(config)
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }
}

/// General configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub hide_dock_icon: bool,
}

fn default_language() -> String {
    "zh-CN".to_string()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            auto_start: false,
            language: default_language(),
            hide_dock_icon: false,
        }
    }
}

/// Hotkey configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_mode")]
    pub mode: String,
    #[serde(default = "default_combo_key")]
    pub combo_key: String,
    #[serde(default = "default_double_tap_key")]
    pub double_tap_key: String,
    #[serde(default = "default_double_tap_interval")]
    pub double_tap_interval: u64,
}

fn default_hotkey_mode() -> String {
    "combo".to_string()
}

fn default_combo_key() -> String {
    "Ctrl+Shift+V".to_string()
}

fn default_double_tap_key() -> String {
    "Ctrl".to_string()
}

fn default_double_tap_interval() -> u64 {
    300
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            mode: default_hotkey_mode(),
            combo_key: default_combo_key(),
            double_tap_key: default_double_tap_key(),
            double_tap_interval: default_double_tap_interval(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// ASR configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfig {
    #[serde(default = "default_true")]
    pub vad_enabled: bool,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self { vad_enabled: true }
    }
}
