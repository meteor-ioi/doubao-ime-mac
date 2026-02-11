#![allow(unexpected_cfgs)]
use crate::data::HotkeyConfig;
use crate::platform::{HotkeyProvider, TextAction};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::thread;
use anyhow::Result;

#[cfg(target_os = "macos")]
mod native {
    use core_graphics::event::{
        CGEvent, CGEventTapLocation, CGKeyCode,
    };
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use foreign_types::ForeignType;

    pub fn insert_text(text: &str) -> anyhow::Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

        // In core-graphics 0.23, we might need to use a different way to send Unicode
        // raw API via an extern block.

        let event = CGEvent::new_keyboard_event(source, 0, true)
            .map_err(|_| anyhow::anyhow!("Failed to create CGEvent"))?;
        
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let event_ref = event.as_ptr();
            CGEventKeyboardSetUnicodeString(event_ref as _, utf16.len(), utf16.as_ptr());
        }
        
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventKeyboardSetUnicodeString(event: *mut libc::c_void, length: libc::size_t, string: *const u16);
    }

    pub fn delete_chars(count: usize) -> anyhow::Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| anyhow::anyhow!("Failed to create CGEventSource"))?;

        const VK_BACKSPACE: CGKeyCode = 51;

        for _ in 0..count {
            if let Ok(event_down) = CGEvent::new_keyboard_event(source.clone(), VK_BACKSPACE, true) {
                event_down.post(CGEventTapLocation::HID);
            }
            if let Ok(event_up) = CGEvent::new_keyboard_event(source.clone(), VK_BACKSPACE, false) {
                event_up.post(CGEventTapLocation::HID);
            }
        }

        Ok(())
    }
}

pub struct MacosTextInserter;

impl MacosTextInserter {
    pub fn new() -> Self {
        Self
    }
}

impl TextAction for MacosTextInserter {
    fn insert(&self, text: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        return native::insert_text(text);
        #[cfg(not(target_os = "macos"))]
        { let _ = text; Ok(()) }
    }

    fn delete_chars(&self, count: usize) -> Result<()> {
        #[cfg(target_os = "macos")]
        return native::delete_chars(count);
        #[cfg(not(target_os = "macos"))]
        { let _ = count; Ok(()) }
    }
}

pub struct MacosHotkeyProvider {
    is_active: Arc<AtomicBool>,
    monitor_handle: Arc<std::sync::Mutex<Option<usize>>>,
    double_tap_key: String,
    double_tap_interval: Duration,
}

impl MacosHotkeyProvider {
    pub fn new(config: &HotkeyConfig) -> Result<Self> {
        Ok(Self {
            is_active: Arc::new(AtomicBool::new(true)),
            monitor_handle: Arc::new(std::sync::Mutex::new(None)),
            double_tap_key: config.double_tap_key.clone(),
            double_tap_interval: Duration::from_millis(config.double_tap_interval),
        })
    }
}

impl HotkeyProvider for MacosHotkeyProvider {
    fn on_trigger(&self, callback: Box<dyn Fn() + Send + Sync + 'static>) {
        let is_active = self.is_active.clone();
        let target_key = self.double_tap_key.to_lowercase();
        let interval = self.double_tap_interval;
        let monitor_handle = self.monitor_handle.clone();

        thread::spawn(move || {
            #[cfg(target_os = "macos")]
            {
                use cocoa::appkit::NSEventMask;
                use cocoa::base::id;
                use objc::{msg_send, sel, sel_impl};
                use std::cell::RefCell;
                use block::ConcreteBlock;

                thread_local! {
                    static LAST_PRESS: RefCell<Option<Instant>> = RefCell::new(None);
                }

                let target_mask: u64 = match target_key.as_str() {
                    "control" | "ctrl" => 262144,
                    "shift" => 131072,
                    "alt" | "option" => 524288,
                    "command" | "cmd" | "meta" => 1048576,
                    _ => 0,
                };

                if target_mask == 0 {
                    return;
                }

                unsafe {
                    let callback = Arc::new(callback);
                    let block = ConcreteBlock::new(move |event: id| {
                        if !is_active.load(Ordering::SeqCst) {
                            return;
                        }
                        let flags: u64 = msg_send![event, modifierFlags];
                        if (flags & target_mask) != 0 {
                            LAST_PRESS.with(|lp| {
                                let mut last = lp.borrow_mut();
                                let now = Instant::now();
                                if let Some(t) = *last {
                                    if now.duration_since(t) <= interval {
                                        callback();
                                        *last = None;
                                        return;
                                    }
                                }
                                *last = Some(now);
                            });
                        }
                    });
                    let block = block.copy();

                    let class = objc::runtime::Class::get("NSEvent").unwrap();
                    let monitor: id = msg_send![class, 
                        addGlobalMonitorForEventsMatchingMask: NSEventMask::NSFlagsChangedMask 
                        handler: &*block];
                    
                    // Store the monitor handle so we can remove it later
                    if !monitor.is_null() {
                        if let Ok(mut handle) = monitor_handle.lock() {
                            *handle = Some(monitor as usize);
                        }
                    }
                }
            }
        });
    }

    fn stop(&self) {
        self.is_active.store(false, Ordering::SeqCst);

        #[cfg(target_os = "macos")]
        {
            use cocoa::base::id;
            use objc::{msg_send, sel, sel_impl};

            if let Ok(mut handle) = self.monitor_handle.lock() {
                if let Some(monitor_ptr) = handle.take() {
                    unsafe {
                        let class = objc::runtime::Class::get("NSEvent").unwrap();
                        let _: () = msg_send![class, removeMonitor: monitor_ptr as id];
                    }
                }
            }
        }
    }
}

impl Drop for MacosHotkeyProvider {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct PlatformImpl;
