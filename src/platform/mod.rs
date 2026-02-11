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

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::PlatformImpl;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::PlatformImpl;

/// Factory for creating platform-specific implementations
pub struct PlatformFactory;

impl PlatformFactory {
    pub fn create_text_action() -> Box<dyn TextAction> {
        #[cfg(target_os = "windows")]
        return Box::new(windows::WindowsTextInserter::new());
        #[cfg(target_os = "macos")]
        return Box::new(macos::MacosTextInserter::new());
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        panic!("Unsupported platform");
    }

    pub fn create_hotkey_provider(
        config: &crate::data::HotkeyConfig,
    ) -> Result<Box<dyn HotkeyProvider>> {
        #[cfg(target_os = "windows")]
        return Ok(Box::new(windows::WindowsHotkeyProvider::new(config)?));
        #[cfg(target_os = "macos")]
        return Ok(Box::new(macos::MacosHotkeyProvider::new(config)?));
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        panic!("Unsupported platform");
    }
}
