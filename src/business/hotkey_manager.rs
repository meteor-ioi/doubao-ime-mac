//! Hotkey Manager
//!
//! Manages global hotkeys for triggering voice input.
//! Uses platform-specific implementation for hotkey listening.
//! Supports runtime reconfiguration via stop+replace strategy.

use crate::data::HotkeyConfig;
use crate::platform::{HotkeyProvider, PlatformFactory};
use anyhow::Result;
use std::sync::{Arc, Mutex};

/// Hotkey manager for global hotkey handling.
///
/// Supports runtime hotkey updates: when `update_config` is called,
/// the old provider is stopped and a new one is created with the updated config.
pub struct HotkeyManager {
    provider: Mutex<Box<dyn HotkeyProvider>>,
    callback: Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync + 'static>>>>,
}

impl HotkeyManager {
    /// Create a new hotkey manager based on configuration
    pub fn new(config: &HotkeyConfig) -> Result<Self> {
        let provider = PlatformFactory::create_hotkey_provider(config)?;
        Ok(Self {
            provider: Mutex::new(provider),
            callback: Arc::new(Mutex::new(None)),
        })
    }

    /// Set callback for when hotkey is triggered.
    /// The callback is stored so it can be re-bound after config updates.
    pub fn on_trigger(&self, callback: Arc<dyn Fn() + Send + Sync + 'static>) {
        // Store the callback for future re-binding
        if let Ok(mut cb) = self.callback.lock() {
            *cb = Some(callback.clone());
        }
        // Bind to current provider
        if let Ok(provider) = self.provider.lock() {
            provider.on_trigger(Box::new(move || callback()));
        }
    }

    /// Update hotkey configuration at runtime.
    /// Stops the old provider and creates a new one with the new config.
    pub fn update_config(&self, config: &HotkeyConfig) -> Result<()> {
        // 1. Stop old provider
        if let Ok(mut provider) = self.provider.lock() {
            provider.stop();

            // 2. Create new provider
            let new_provider = PlatformFactory::create_hotkey_provider(config)?;

            // 3. Re-bind stored callback
            if let Ok(cb) = self.callback.lock() {
                if let Some(ref callback) = *cb {
                    let callback_clone = callback.clone();
                    new_provider.on_trigger(Box::new(move || callback_clone()));
                }
            }

            // 4. Replace provider
            *provider = new_provider;
        }

        tracing::info!("Hotkey config updated successfully");
        Ok(())
    }

    /// Stop the hotkey manager
    pub fn stop(&self) {
        if let Ok(provider) = self.provider.lock() {
            provider.stop();
        }
    }
}
