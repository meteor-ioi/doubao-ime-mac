use anyhow::Result;

/// Trait for platform-specific text actions
pub trait TextAction: Send + Sync {
    /// Insert text into the currently focused window
    fn insert(&self, text: &str) -> Result<()>;
    /// Delete specified number of characters
    fn delete_chars(&self, count: usize) -> Result<()>;
}

/// Trait for platform-specific hotkey management
pub trait HotkeyProvider: Send + Sync {
    /// Start listening for hotkeys
    fn on_trigger(&self, callback: Box<dyn Fn() + Send + Sync + 'static>);
    /// Stop listening
    fn stop(&self);
}

pub mod macos;
pub use macos::PlatformImpl;

/// Factory for creating platform-specific implementations
pub struct PlatformFactory;

impl PlatformFactory {
    pub fn create_text_action() -> Box<dyn TextAction> {
        Box::new(macos::MacosTextInserter::new())
    }

    pub fn create_hotkey_provider(
        config: &crate::data::HotkeyConfig,
    ) -> Result<Box<dyn HotkeyProvider>> {
        Ok(Box::new(macos::MacosHotkeyProvider::new(config)?))
    }
}
