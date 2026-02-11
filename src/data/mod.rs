//! Data module for configuration and credential management

mod config;
mod credential;

pub use config::{AppConfig, AsrConfig, GeneralConfig, HotkeyConfig};
pub use credential::CredentialStore;
