use crate::platform::TextAction;
use anyhow::Result;
use std::mem::size_of;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY, VK_BACK,
};

pub struct WindowsTextInserter;

impl WindowsTextInserter {
    pub fn new() -> Self {
        Self
    }

    /// Create a Unicode character input
    fn create_unicode_input(&self, ch: u16, key_down: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: if key_down {
                        KEYEVENTF_UNICODE
                    } else {
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// Create a virtual key input
    fn create_key_input(&self, vk: VIRTUAL_KEY, key_down: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if key_down {
                        windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// Send inputs using Windows SendInput API
    fn send_inputs(&self, inputs: &[INPUT]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }

        let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };

        if sent != inputs.len() as u32 {
            tracing::warn!("SendInput sent {} of {} inputs", sent, inputs.len());
        }

        Ok(())
    }
}

impl TextAction for WindowsTextInserter {
    fn insert(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        let mut inputs: Vec<INPUT> = Vec::new();

        for ch in text.encode_utf16() {
            // Key down
            inputs.push(self.create_unicode_input(ch, true));
            // Key up
            inputs.push(self.create_unicode_input(ch, false));
        }

        self.send_inputs(&inputs)?;
        Ok(())
    }

    fn delete_chars(&self, count: usize) -> Result<()> {
        if count == 0 {
            return Ok(());
        }

        let mut inputs: Vec<INPUT> = Vec::new();

        for _ in 0..count {
            // Backspace key down
            inputs.push(self.create_key_input(VK_BACK, true));
            // Backspace key up
            inputs.push(self.create_key_input(VK_BACK, false));
        }

        self.send_inputs(&inputs)?;
        Ok(())
    }
}

pub struct PlatformImpl;

use crate::data::HotkeyConfig;
use crate::platform::HotkeyProvider;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub struct WindowsHotkeyProvider {
    _manager: Option<GlobalHotKeyManager>,
    mode: String,
    double_tap_interval: Duration,
    double_tap_key: String,
    is_active: Arc<AtomicBool>,
    hook_thread_id: Arc<std::sync::Mutex<Option<u32>>>,
}

impl WindowsHotkeyProvider {
    pub fn new(config: &HotkeyConfig) -> Result<Self> {
        let manager = GlobalHotKeyManager::new()
            .map_err(|e| anyhow::anyhow!("Failed to create hotkey manager: {}", e))?;

        if config.mode == "combo" {
            let hotkey = parse_combo_key(&config.combo_key)?;
            manager
                .register(hotkey)
                .map_err(|e| anyhow::anyhow!("Failed to register hotkey: {}", e))?;
        } else {
            let key_lower = config.double_tap_key.to_lowercase();
            if key_lower != "ctrl" && key_lower != "shift" && key_lower != "alt" {
                let hotkey = HotKey::new(None, parse_key_code(&config.double_tap_key)?);
                manager
                    .register(hotkey)
                    .map_err(|e| anyhow::anyhow!("Failed to register hotkey: {}", e))?;
            }
        }

        Ok(Self {
            _manager: Some(manager),
            mode: config.mode.clone(),
            double_tap_interval: Duration::from_millis(config.double_tap_interval),
            double_tap_key: config.double_tap_key.clone(),
            is_active: Arc::new(AtomicBool::new(true)),
            hook_thread_id: Arc::new(std::sync::Mutex::new(None)),
        })
    }
}

impl HotkeyProvider for WindowsHotkeyProvider {
    fn on_trigger(&self, callback: Box<dyn Fn() + Send + Sync + 'static>) {
        let mode = self.mode.clone();
        let double_tap_interval = self.double_tap_interval;
        let double_tap_key = self.double_tap_key.clone();
        let is_active = self.is_active.clone();
        let hook_thread_id = self.hook_thread_id.clone();
        let callback = Arc::new(callback);

        let key_lower = double_tap_key.to_lowercase();
        let use_keyboard_hook = mode == "double_tap"
            && (key_lower == "ctrl" || key_lower == "shift" || key_lower == "alt");

        if use_keyboard_hook {
            let callback_clone = callback.clone();
            thread::spawn(move || {
                #[cfg(windows)]
                {
                    use windows::Win32::System::Threading::GetCurrentThreadId;
                    let tid = unsafe { GetCurrentThreadId() };
                    if let Ok(mut lock) = hook_thread_id.lock() {
                        *lock = Some(tid);
                    }
                }

                run_modifier_double_tap_hook(
                    key_lower,
                    double_tap_interval,
                    is_active,
                    callback_clone,
                );
            });
        } else {
            thread::spawn(move || {
                let receiver = GlobalHotKeyEvent::receiver();
                let mut last_press_time: Option<Instant> = None;

                loop {
                    if !is_active.load(Ordering::SeqCst) {
                        break;
                    }

                    // Use try_recv with timeout to allow checking is_active
                    if let Ok(_event) = receiver.recv_timeout(Duration::from_millis(200)) {
                        if mode == "combo" {
                            callback();
                        } else {
                            let now = Instant::now();
                            if let Some(last) = last_press_time {
                                let elapsed = now.duration_since(last);
                                if elapsed <= double_tap_interval {
                                    callback();
                                    last_press_time = None;
                                    continue;
                                }
                            }
                            last_press_time = Some(now);
                        }
                    }
                }
            });
        }
    }

    fn stop(&self) {
        self.is_active.store(false, Ordering::SeqCst);

        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            if let Ok(lock) = self.hook_thread_id.lock() {
                if let Some(tid) = *lock {
                    unsafe {
                        let _ = PostThreadMessageW(
                            tid,
                            WM_QUIT,
                            windows::Win32::Foundation::WPARAM(0),
                            windows::Win32::Foundation::LPARAM(0),
                        );
                    }
                }
            }
        }
    }
}

impl Drop for WindowsHotkeyProvider {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Windows keyboard hook for modifier key double-tap detection
fn run_modifier_double_tap_hook<F>(
    key: String,
    interval: Duration,
    is_active: Arc<AtomicBool>,
    callback: Arc<F>,
) where
    F: Fn() + Send + Sync + 'static,
{
    use std::cell::RefCell;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYUP, WM_SYSKEYUP,
    };

    // Determine which virtual keys to watch
    let target_vks: Vec<u16> = match key.as_str() {
        "ctrl" => vec![VK_CONTROL.0, VK_LCONTROL.0, VK_RCONTROL.0],
        "shift" => vec![VK_LSHIFT.0, VK_RSHIFT.0],
        "alt" => vec![VK_LMENU.0, VK_RMENU.0],
        _ => vec![],
    };

    if target_vks.is_empty() {
        tracing::error!("Unknown modifier key: {}", key);
        return;
    }

    tracing::info!("Starting keyboard hook for double-tap {} detection", key);

    // Thread-local state for hook callback
    thread_local! {
        static HOOK_STATE: RefCell<Option<HookState>> = RefCell::new(None);
    }

    struct HookState {
        target_vks: Vec<u16>,
        interval: Duration,
        last_release: Option<Instant>,
        callback: Arc<dyn Fn() + Send + Sync>,
        is_active: Arc<AtomicBool>,
    }

    // Initialize thread-local state
    HOOK_STATE.with(|state| {
        *state.borrow_mut() = Some(HookState {
            target_vks,
            interval,
            last_release: None,
            callback: callback as Arc<dyn Fn() + Send + Sync>,
            is_active,
        });
    });

    // Low-level keyboard hook procedure
    unsafe extern "system" fn keyboard_hook_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 {
            let kb_struct = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk_code = kb_struct.vkCode as u16;
            let is_key_up = wparam.0 as u32 == WM_KEYUP || wparam.0 as u32 == WM_SYSKEYUP;

            HOOK_STATE.with(|state| {
                if let Some(ref mut hook_state) = *state.borrow_mut() {
                    if hook_state.is_active.load(Ordering::SeqCst)
                        && hook_state.target_vks.contains(&vk_code)
                        && is_key_up
                    {
                        let now = Instant::now();
                        if let Some(last) = hook_state.last_release {
                            let elapsed = now.duration_since(last);
                            if elapsed <= hook_state.interval {
                                // Double-tap detected!
                                tracing::info!("Double-tap detected!");
                                (hook_state.callback)();
                                hook_state.last_release = None;
                            } else {
                                hook_state.last_release = Some(now);
                            }
                        } else {
                            hook_state.last_release = Some(now);
                        }
                    }
                }
            });
        }

        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    // Install the hook
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0) };

    match hook {
        Ok(h) => {
            tracing::info!("Keyboard hook installed successfully");

            // Message loop to keep hook alive
            let mut msg = MSG::default();
            unsafe {
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    DispatchMessageW(&msg);
                }
            }

            // Cleanup
            let _ = unsafe { UnhookWindowsHookEx(h) };
            tracing::info!("Keyboard hook uninstalled");
        }
        Err(e) => {
            tracing::error!("Failed to install keyboard hook: {:?}", e);
        }
    }
}

/// Parse a combo key string like "Ctrl+Shift+V"
fn parse_combo_key(key_str: &str) -> Result<HotKey> {
    let parts: Vec<&str> = key_str.split('+').map(|s| s.trim()).collect();

    let mut modifiers = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for part in parts {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" => modifiers |= Modifiers::ALT,
            "super" | "win" | "meta" => modifiers |= Modifiers::SUPER,
            _ => {
                key_code = Some(parse_key_code(part)?);
            }
        }
    }

    let code = key_code.ok_or_else(|| anyhow::anyhow!("No key specified in combo: {}", key_str))?;

    Ok(HotKey::new(Some(modifiers), code))
}

/// Parse a key code from string
fn parse_key_code(key: &str) -> Result<Code> {
    let code = match key.to_uppercase().as_str() {
        "A" => Code::KeyA,
        "B" => Code::KeyB,
        "C" => Code::KeyC,
        "D" => Code::KeyD,
        "E" => Code::KeyE,
        "F" => Code::KeyF,
        "G" => Code::KeyG,
        "H" => Code::KeyH,
        "I" => Code::KeyI,
        "J" => Code::KeyJ,
        "K" => Code::KeyK,
        "L" => Code::KeyL,
        "M" => Code::KeyM,
        "N" => Code::KeyN,
        "O" => Code::KeyO,
        "P" => Code::KeyP,
        "Q" => Code::KeyQ,
        "R" => Code::KeyR,
        "S" => Code::KeyS,
        "T" => Code::KeyT,
        "U" => Code::KeyU,
        "V" => Code::KeyV,
        "W" => Code::KeyW,
        "X" => Code::KeyX,
        "Y" => Code::KeyY,
        "Z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "SPACE" => Code::Space,
        "ENTER" | "RETURN" => Code::Enter,
        "ESCAPE" | "ESC" => Code::Escape,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        _ => return Err(anyhow::anyhow!("Unknown key: {}", key)),
    };

    Ok(code)
}
