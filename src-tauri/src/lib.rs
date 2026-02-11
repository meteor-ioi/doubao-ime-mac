use doubao_voice_input::{
    AppConfig, AsrClient, AudioCapture, CredentialStore, HotkeyManager, TextInserter,
    VoiceController,
};
use image::GenericImageView;
use serde_json::json;
use std::sync::Arc;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, Runtime, State,
};
use tokio::sync::Mutex;

/// Application state managed by Tauri, accessible from commands.
struct AppState {
    hotkey_manager: Arc<HotkeyManager>,
}

#[tauri::command]
async fn get_config() -> Result<AppConfig, String> {
    AppConfig::load_or_default().map_err(|e| e.to_string())
}

#[tauri::command]
fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    return macos_ext::is_accessibility_enabled();
    #[cfg(not(target_os = "macos"))]
    true
}

#[tauri::command]
async fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())?;

    // Apply hotkey changes immediately
    state
        .hotkey_manager
        .update_config(&config.hotkey)
        .map_err(|e| e.to_string())?;

    // Apply side effects
    #[cfg(target_os = "macos")]
    {
        macos_ext::set_dock_visible(!config.general.hide_dock_icon);
        // Important: Changing ActivationPolicy on macOS can hide all app windows.
        // We must ensure the settings window stays visible if it was open.
        if let Some(window) = app.get_webview_window("settings") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod macos_ext {
    use cocoa::appkit::NSWindowCollectionBehavior;
    use cocoa::base::{id, nil, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use tauri::{PhysicalPosition, Runtime, WebviewWindow};

    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use std::os::raw::c_void;
    use std::ptr;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> *mut c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *const c_void,
            value: *mut *mut c_void,
        ) -> i32;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: *mut c_void,
            parameterized_attribute: *const c_void,
            parameter: *mut c_void,
            result: *mut *mut c_void,
        ) -> i32;
        fn AXValueGetValue(value: *mut c_void, the_type: u32, value_ptr: *mut c_void) -> u8;
        fn CFRelease(cf: *const c_void);
        fn AXIsProcessTrusted() -> bool;
    }

    pub fn is_accessibility_enabled() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    const AX_VALUE_TYPE_CGRECT: u32 = 3;

    fn cfstr_retained(s: &str) -> *const c_void {
        let cf = CFString::new(s);
        let ptr = cf.as_concrete_TypeRef() as *const c_void;
        unsafe {
            let _: *const c_void = msg_send![ptr as id, retain];
        }
        ptr
    }

    /// Get the caret (text cursor) position via macOS Accessibility API.
    /// Returns Some((x, y)) in screen coordinates (top-left origin).
    pub fn get_caret_position() -> Option<(f64, f64)> {
        unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if system_wide.is_null() {
                return None;
            }

            // Get focused UI element
            let attr_focused = cfstr_retained("AXFocusedUIElement");
            let mut focused_el: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system_wide, attr_focused, &mut focused_el);
            CFRelease(attr_focused);
            CFRelease(system_wide);
            if err != 0 || focused_el.is_null() {
                return None;
            }

            // Get selected text range
            let attr_range = cfstr_retained("AXSelectedTextRange");
            let mut range_val: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(focused_el, attr_range, &mut range_val);
            CFRelease(attr_range);
            if err != 0 || range_val.is_null() {
                CFRelease(focused_el);
                return None;
            }

            // Get bounds for that range
            let attr_bounds = cfstr_retained("AXBoundsForRange");
            let mut bounds_val: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyParameterizedAttributeValue(
                focused_el,
                attr_bounds,
                range_val,
                &mut bounds_val,
            );
            CFRelease(attr_bounds);
            CFRelease(range_val);
            CFRelease(focused_el);
            if err != 0 || bounds_val.is_null() {
                return None;
            }

            // Extract CGRect from AXValue
            let mut rect = CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(0.0, 0.0));
            let ok = AXValueGetValue(
                bounds_val,
                AX_VALUE_TYPE_CGRECT,
                &mut rect as *mut CGRect as *mut c_void,
            );
            CFRelease(bounds_val);
            if ok == 0 {
                return None;
            }

            // AX coords: origin top-left of primary screen
            Some((rect.origin.x, rect.origin.y + rect.size.height))
        }
    }

    pub fn setup_panel<R: Runtime>(window: &WebviewWindow<R>) {
        if let Ok(ns_window) = window.ns_window() {
            let ns_window = ns_window as id;
            unsafe {
                // Ensure window is borderless and transparent
                let _: () = msg_send![ns_window, setOpaque: NO];
                let clear_color: id = msg_send![class!(NSColor), clearColor];
                let _: () = msg_send![ns_window, setBackgroundColor: clear_color];
                let _: () = msg_send![ns_window, setHasShadow: NO];
                let _: () = msg_send![ns_window, setLevel: 8]; // kCGStatusWindowLevel
                let _: () = msg_send![ns_window, setIgnoresMouseEvents: YES];

                // Collection behavior
                let collection_behavior: u64 = (1 << 0) | // NSWindowCollectionBehaviorCanJoinAllSpaces
                    (1 << 7); // NSWindowCollectionBehaviorFullScreenAuxiliary
                let _: () = msg_send![ns_window, setCollectionBehavior: collection_behavior];
            }
        }
    }

    /// Move indicator below the text caret. Returns true if caret found.
    pub fn update_position_to_caret<R: Runtime>(window: &WebviewWindow<R>) -> bool {
        if let Some((x, y)) = get_caret_position() {
            let _ = window.set_position(PhysicalPosition::new(x as i32 - 24, y as i32 + 4));
            true
        } else {
            false
        }
    }

    pub fn set_dock_visible(visible: bool) {
        unsafe {
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let policy = if visible { 0 } else { 1 };
            let _: () = msg_send![app, setActivationPolicy: policy];
        }
    }

    pub fn refresh_indicator<R: Runtime>(window: &WebviewWindow<R>) {
        let w = window.clone();
        let _ = window.run_on_main_thread(move || {
            let _ = update_position_to_caret(&w);
            if let Ok(ns_window) = w.ns_window() {
                unsafe {
                    let _: () = msg_send![ns_window as id, orderFront: nil];
                }
            }
        });
    }
}

fn play_sound(path: &str) {
    let path = path.to_string();
    std::thread::spawn(move || {
        let _ = std::process::Command::new("afplay").arg(path).status();
    });
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            check_accessibility
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Initializing logic in a background task to avoid blocking setup
            tauri::async_runtime::spawn(async move {
                if let Err(e) = init_core_logic(handle).await {
                    eprintln!("Failed to initialize core logic: {}", e);
                }
            });

            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                macos_ext::setup_panel(&window);
            }

            if let Some(settings_window) = app.get_webview_window("settings") {
                let w = settings_window.clone();
                settings_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            let mut icon =
                image::load_from_memory(include_bytes!("../icons/TrayIconTemplate@2x.png"))
                    .expect("Failed to load tray icon");
            let (width, height) = icon.dimensions();

            let mut rgba_img = icon.into_rgba8();
            for pixel in rgba_img.pixels_mut() {
                if pixel[3] > 0 {
                    pixel[0] = 255;
                    pixel[1] = 255;
                    pixel[2] = 255;
                }
            }
            let rgba = rgba_img.into_vec();
            let tray_icon = tauri::image::Image::new_owned(rgba, width, height);

            // Tray Menu items (Scheme B: Control Center Style)
            let config = AppConfig::load_or_default().unwrap_or_default();

            let status_i = MenuItem::with_id(app, "status", "状态: 就绪", false, None::<&str>)?;
            let show_i = CheckMenuItem::with_id(
                app,
                "show_main",
                "显示指示器",
                true,
                true, // Initial state set to true as requested
                None::<&str>,
            )?;
            let dock_i = CheckMenuItem::with_id(
                app,
                "toggle_dock",
                "隐藏应用图标",
                true,
                config.general.hide_dock_icon,
                None::<&str>,
            )?;
            let autostart_i = CheckMenuItem::with_id(
                app,
                "toggle_autostart",
                "开机自动启动",
                true,
                config.general.auto_start,
                None::<&str>,
            )?;
            let settings_i = MenuItem::with_id(app, "settings", "偏好设置...", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &status_i,
                    &PredefinedMenuItem::separator(app)?,
                    &show_i,
                    &PredefinedMenuItem::separator(app)?,
                    &dock_i,
                    &autostart_i,
                    &PredefinedMenuItem::separator(app)?,
                    &settings_i,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_i,
                ],
            )?;

            // Apply initial dock visibility
            #[cfg(target_os = "macos")]
            macos_ext::set_dock_visible(!config.general.hide_dock_icon);

            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "show_main" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let is_visible = w.is_visible().unwrap_or(false);
                            if is_visible {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }

                            // Update check state
                            if let Some(item) = app.menu().and_then(|m| m.get("show_main")) {
                                if let MenuItemKind::Check(check_item) = item {
                                    let _ = check_item.set_checked(!is_visible);
                                }
                            }
                        }
                    }
                    "toggle_dock" => {
                        let mut config = AppConfig::load_or_default().unwrap_or_default();
                        config.general.hide_dock_icon = !config.general.hide_dock_icon;
                        let _ = config.save();
                        #[cfg(target_os = "macos")]
                        {
                            macos_ext::set_dock_visible(!config.general.hide_dock_icon);
                            // Reshow settings window because setActivationPolicy hides all windows
                            if let Some(w) = app.get_webview_window("settings") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        // Update check state
                        if let Some(item) = app.menu().and_then(|m| m.get("toggle_dock")) {
                            if let MenuItemKind::Check(check_item) = item {
                                let _ = check_item.set_checked(config.general.hide_dock_icon);
                            }
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn init_core_logic<R: Runtime>(handle: AppHandle<R>) -> anyhow::Result<()> {
    // 1. Load config
    let config = AppConfig::load_or_default()?;

    // 2. Credentials
    let credential_store = CredentialStore::new(&config)?;
    let credentials = credential_store.ensure_credentials().await?;

    // 3. Components
    let audio_capture = Arc::new(AudioCapture::new()?);
    let text_inserter = Arc::new(TextInserter::new());
    let asr_client = Arc::new(AsrClient::new(credentials));

    let voice_controller = Arc::new(Mutex::new(VoiceController::new(
        asr_client,
        audio_capture,
        text_inserter,
    )));

    // 4. Hotkeys
    let hotkey_manager = Arc::new(HotkeyManager::new(&config.hotkey)?);

    // Set up hotkey callback
    let vc_clone = voice_controller.clone();
    let handle_clone = handle.clone();

    // Set up ASR result callback to emit to frontend
    {
        let h = handle.clone();
        let mut vc_lock = voice_controller.lock().await;
        vc_lock.set_on_result(move |text, is_final| {
            #[cfg(target_os = "macos")]
            if let Some(w) = h.get_webview_window("main") {
                macos_ext::refresh_indicator(&w);
            }
            let status = if is_final { "processing" } else { "recording" };
            let _ = h.emit(
                "asr-status",
                json!({
                    "status": status,
                    "text": text
                }),
            );
        });
    }

    let callback: Arc<dyn Fn() + Send + Sync + 'static> = Arc::new(move || {
        let vc = vc_clone.clone();
        let h = handle_clone.clone();
        tauri::async_runtime::spawn(async move {
            let mut vc_lock = vc.lock().await;
            let was_recording = vc_lock.is_recording();

            if let Err(e) = vc_lock.toggle().await {
                eprintln!("Toggle error: {}", e);
            }

            let is_recording = vc_lock.is_recording();

            // Audio cues
            if is_recording && !was_recording {
                play_sound("/System/Library/Sounds/Tink.aiff");
            } else if !is_recording && was_recording {
                play_sound("/System/Library/Sounds/Pop.aiff");
            }

            // Window visibility and position
            if let Some(w) = h.get_webview_window("main") {
                if is_recording {
                    #[cfg(target_os = "macos")]
                    {
                        macos_ext::refresh_indicator(&w);
                    }
                } else {
                    let _ = w.hide();
                }
            }

            // Notify frontend
            let _ = h.emit(
                "asr-status",
                json!({
                    "status": if is_recording { "recording" } else { "idle" },
                    "text": ""
                }),
            );
        });
    });
    hotkey_manager.on_trigger(callback);

    // Register AppState for access from commands (e.g., save_config)
    handle.manage(AppState {
        hotkey_manager: hotkey_manager.clone(),
    });

    // TODO: We need a way to get ASR interim results from VoiceController
    // and emit them to the frontend.
    // For now, it will only show "Listening..." based on status.

    Ok(())
}
