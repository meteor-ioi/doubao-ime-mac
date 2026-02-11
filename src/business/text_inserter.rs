//! Text Inserter abstraction
//!
//! Inserts text into the currently focused window using platform-specific simulation.

use crate::platform::{PlatformFactory, TextAction};
use anyhow::Result;

/// Text inserter service using platform-specific implementation
pub struct TextInserter {
    inner: Box<dyn TextAction>,
}

impl TextInserter {
    /// Create a new text inserter
    pub fn new() -> Self {
        Self {
            inner: PlatformFactory::create_text_action(),
        }
    }

    /// Insert text into the currently focused window
    pub fn insert(&self, text: &str) -> Result<()> {
        self.inner.insert(text)
    }

    /// Delete specified number of characters (simulate backspace)
    pub fn delete_chars(&self, count: usize) -> Result<()> {
        self.inner.delete_chars(count)
    }
}

impl Default for TextInserter {
    fn default() -> Self {
        Self::new()
    }
}
