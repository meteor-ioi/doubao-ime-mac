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
    AppHandle, Emitter, Manager, Runtime, State, Wry,
};
use tokio::sync::Mutex;

/// Application state managed by Tauri, accessible from commands.
struct AppState {
    hotkey_manager: Arc<HotkeyManager>,
}

struct TrayMenu(pub Menu<Wry>);

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
    tray_menu: State<'_, TrayMenu>,
    config: AppConfig,
) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())?;

    // Apply hotkey changes immediately
    state
        .hotkey_manager
        .update_config(&config.hotkey)
        .map_err(|e| e.to_string())?;

    // Apply auto-start capability
    use tauri_plugin_autostart::ManagerExt;
    let autostart_manager = app.autolaunch();
    if config.general.auto_start {
        let _ = autostart_manager.enable();
    } else {
        let _ = autostart_manager.disable();
    }

    // Sync Tray Menu State
    // Sync Tray Menu State
    // Access tray menu via managed state
    let menu = &tray_menu.0;
    if let Some(MenuItemKind::Check(item)) = menu.get("toggle_dock") {
        let _ = item.set_checked(config.general.hide_dock_icon);
    }
    if let Some(MenuItemKind::Check(item)) = menu.get("toggle_autostart") {
        let _ = item.set_checked(config.general.auto_start);
    }

    // Apply side effects
    #[cfg(target_os = "macos")]
    {
        macos_ext::set_dock_visible(!config.general.hide_dock_icon);
        // Important: Changing ActivationPolicy on macOS can hide all app windows.
        // We must ensure the settings window stays visible if it was open.
        if let Some(window) = app.get_webview_window("settings") {
            if window.is_visible().unwrap_or(false) {
                let _ = window.set_focus();
            }
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod macos_ext {
    use cocoa::base::{id, nil, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use tauri::{Runtime, WebviewWindow};

    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_graphics::geometry::{CGPoint, CGSize};
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
        fn CFStringGetLength(theString: *const c_void) -> i64;
        fn CFStringGetCharacters(theString: *const c_void, range: CFRange, buffer: *mut u16);
    }

    type CFStringRef = *const c_void;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CFRange {
        location: i64,
        length: i64,
    }

    pub fn is_accessibility_enabled() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn get_focused_pid() -> Option<i32> {
        let _pid: i32 = 0;
        unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if system_wide.is_null() {
                return None;
            }

            let attr_app = cfstr_retained("AXFocusedApplication");
            let mut app_el: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system_wide, attr_app, &mut app_el);
            CFRelease(attr_app);
            CFRelease(system_wide);

            if err == 0 && !app_el.is_null() {
                // Use AXUIElementGetPid
                // We need to link and define it first, or use the NSWorkspace approach.
                // Since we have AXUIElement, let's use the C API if available,
                // BUT linking AXUIElementGetPid requires declaring it.
                // Let's use NSWorkspace for simplicity as we already have objc setup.
                CFRelease(app_el);

                let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
                let front_app: id = msg_send![workspace, frontmostApplication];
                if front_app != nil {
                    let p: i32 = msg_send![front_app, processIdentifier];
                    return Some(p);
                }
                return None;
            }
        }
        None
    }

    const AX_VALUE_TYPE_CGPOINT: u32 = 1;
    const AX_VALUE_TYPE_CGSIZE: u32 = 2;
    const AX_VALUE_TYPE_CGRECT: u32 = 3;

    fn cfstr_retained(s: &str) -> *const c_void {
        let cf = CFString::new(s);
        let ptr = cf.as_concrete_TypeRef() as *const c_void;
        unsafe {
            let _: *const c_void = msg_send![ptr as id, retain];
        }
        ptr
    }

    /// Get the target position for the indicator.
    /// Returns a coordinate to the right of the window's traffic light buttons.
    pub fn get_indicator_position() -> Option<(f64, f64)> {
        unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if system_wide.is_null() {
                return None;
            }

            // Step 1: SystemWide → AXFocusedApplication
            let attr_app = cfstr_retained("AXFocusedApplication");
            let mut app_el: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system_wide, attr_app, &mut app_el);
            CFRelease(attr_app);
            CFRelease(system_wide);

            if err != 0 || app_el.is_null() {
                eprintln!(
                    "[Indicator] Cannot get AXFocusedApplication, AXError={}",
                    err
                );
                return None;
            }

            // Step 2: FocusedApplication → AXFocusedWindow
            let attr_window = cfstr_retained("AXFocusedWindow");
            let mut window_el: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(app_el, attr_window, &mut window_el);
            CFRelease(attr_window);
            CFRelease(app_el);

            if err != 0 || window_el.is_null() {
                eprintln!("[Indicator] Cannot get AXFocusedWindow, AXError={}", err);
                return None;
            }

            // Step 3: Get window position as fallback
            let attr_pos = cfstr_retained("AXPosition");
            let mut w_pos_val: *mut c_void = ptr::null_mut();
            let err_w_pos = AXUIElementCopyAttributeValue(window_el, attr_pos, &mut w_pos_val);
            CFRelease(attr_pos);

            let mut window_origin = CGPoint::new(0.0, 0.0);
            if err_w_pos == 0 && !w_pos_val.is_null() {
                AXValueGetValue(
                    w_pos_val,
                    AX_VALUE_TYPE_CGPOINT,
                    &mut window_origin as *mut CGPoint as *mut c_void,
                );
                CFRelease(w_pos_val);
            }

            // Step 4: Dynamically find the traffic light buttons
            // AXFullScreenButton (green) is the rightmost; try it first for best alignment
            let button_attrs = [
                "AXFullScreenButton",
                "AXZoomButton",
                "AXMinimizeButton",
                "AXCloseButton",
            ];
            for attr_name in button_attrs {
                let attr = cfstr_retained(attr_name);
                let mut button_el: *mut c_void = ptr::null_mut();
                let err = AXUIElementCopyAttributeValue(window_el, attr, &mut button_el);
                CFRelease(attr);

                if err == 0 && !button_el.is_null() {
                    let a_pos = cfstr_retained("AXPosition");
                    let a_size = cfstr_retained("AXSize");
                    let mut b_pos_ptr: *mut c_void = ptr::null_mut();
                    let mut b_size_ptr: *mut c_void = ptr::null_mut();
                    let e1 = AXUIElementCopyAttributeValue(button_el, a_pos, &mut b_pos_ptr);
                    let e2 = AXUIElementCopyAttributeValue(button_el, a_size, &mut b_size_ptr);
                    CFRelease(a_pos);
                    CFRelease(a_size);
                    CFRelease(button_el);

                    if e1 == 0 && !b_pos_ptr.is_null() && e2 == 0 && !b_size_ptr.is_null() {
                        let mut b_pos = CGPoint::new(0.0, 0.0);
                        let mut b_size = CGSize::new(0.0, 0.0);
                        AXValueGetValue(
                            b_pos_ptr,
                            AX_VALUE_TYPE_CGPOINT,
                            &mut b_pos as *mut CGPoint as *mut c_void,
                        );
                        AXValueGetValue(
                            b_size_ptr,
                            AX_VALUE_TYPE_CGSIZE,
                            &mut b_size as *mut CGSize as *mut c_void,
                        );
                        CFRelease(b_pos_ptr);
                        CFRelease(b_size_ptr);
                        CFRelease(window_el);

                        // Place indicator so mic icon center aligns with button center
                        // Window is 32pt height (changed from 36)
                        // Button center at b_pos.y + b_size.height/2
                        // To center window: y = button_center_y - window_height/2
                        let x = b_pos.x + b_size.width + 10.0;
                        let button_center_y = b_pos.y + b_size.height / 2.0;
                        let y = button_center_y - 16.0; // 16 = half of window height (32/2)

                        // eprintln!(
                        //     "[Indicator] Aligned to {} at ({:.0}, {:.0}), btn_center_y={:.0}",
                        //     attr_name, x, y, button_center_y
                        // );
                        return Some((x, y));
                    }
                }
            }

            // Fallback: title bar area (window origin + fixed offset)
            CFRelease(window_el);
            eprintln!(
                "[Indicator] No buttons found, using window origin fallback ({:.0}, {:.0})",
                window_origin.x, window_origin.y
            );
            Some((window_origin.x + 80.0, window_origin.y + 6.0))
        }
    }

    /// Get the caret position (logic from implementation plan)
    pub fn get_caret_rect() -> Option<core_graphics::geometry::CGRect> {
        use core_graphics::geometry::CGRect;
        unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if system_wide.is_null() {
                return None;
            }

            // 1. Get Focused Application
            let attr_app = cfstr_retained("AXFocusedApplication");
            let mut app_el: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(system_wide, attr_app, &mut app_el);
            CFRelease(attr_app);
            CFRelease(system_wide);

            if err != 0 || app_el.is_null() {
                eprintln!("[Caret] Failed to get FocusedApplication: {}", err);
                return None;
            }

            // 2. Get Focused UI Element
            let attr_focused_el = cfstr_retained("AXFocusedUIElement");
            let mut focused_el: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyAttributeValue(app_el, attr_focused_el, &mut focused_el);
            CFRelease(attr_focused_el);
            CFRelease(app_el);

            if err != 0 || focused_el.is_null() {
                eprintln!("[Caret] Failed to get FocusedUIElement: {}", err);
                return None;
            }

            // 3. Get Selected Text Range
            let attr_selected_text_range = cfstr_retained("AXSelectedTextRange");
            let mut range_val: *mut c_void = ptr::null_mut();
            let err =
                AXUIElementCopyAttributeValue(focused_el, attr_selected_text_range, &mut range_val);
            CFRelease(attr_selected_text_range);

            if err != 0 || range_val.is_null() {
                eprintln!(
                    "[Caret] No AXSelectedTextRange on this element (err={})",
                    err
                );
                CFRelease(focused_el);
                return None;
            }

            // 4. Get Bounds for Range
            // First we need to get the range value out of the AXValue
            // BUT AXUIElementCopyParameterizedAttributeValue expects the AXValue as the parameter directly?
            // Yes, "The parameter ... is an AXValue representing the range of characters"

            let attr_bounds_for_range = cfstr_retained("AXBoundsForRange");
            let mut bounds_val: *mut c_void = ptr::null_mut();
            let err = AXUIElementCopyParameterizedAttributeValue(
                focused_el,
                attr_bounds_for_range,
                range_val, // The range AXValue
                &mut bounds_val,
            );
            CFRelease(attr_bounds_for_range);
            CFRelease(range_val);
            CFRelease(focused_el);

            if err != 0 || bounds_val.is_null() {
                eprintln!("[Caret] AXBoundsForRange failed: {}", err);
                return None;
            }

            // 5. Convert AXValue to CGRect
            let mut rect = CGRect::new(
                &core_graphics::geometry::CGPoint::new(0.0, 0.0),
                &core_graphics::geometry::CGSize::new(0.0, 0.0),
            );

            let success = AXValueGetValue(
                bounds_val,
                AX_VALUE_TYPE_CGRECT,
                &mut rect as *mut _ as *mut c_void,
            );
            CFRelease(bounds_val);

            if success != 0 {
                // AXValueGetValue returns scalar boolean (1 for success?)
                Some(rect)
            } else {
                None
            }
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
    /// Falls back to screen center if caret detection fails.
    pub fn update_position_to_caret<R: Runtime>(window: &WebviewWindow<R>) -> bool {
        if let Some(caret_rect) = get_caret_rect() {
            eprintln!("[Caret] Detected caret rect: {:?}", caret_rect);
            // Attempt to use caret position
            if let Ok(Some(monitor)) = window
                .current_monitor()
                .or_else(|_| window.primary_monitor())
            {
                let _scale_factor = monitor.scale_factor();
                // Get primary monitor height for coordinate conversion (macOS bottom-left -> top-left)
                // Note: tauri::Monitor doesn't easily give us the "primary" screen height in logical pixels relative to global.
                // We'll use the current monitor, assuming the window is on it or we want it there.
                // But AX coordinates are global (0,0 is primary bottom-left).

                let cg_y = caret_rect.origin.y;
                let cg_height = caret_rect.size.height;

                let y_top_left_global = cg_y;

                // Now we have global top-left coordinates.
                // Target X: centered on caret
                let x_global = caret_rect.origin.x + (caret_rect.size.width / 2.0);

                // Target Y: slightly below caret
                let y_global = y_top_left_global + cg_height + 5.0; // 5px padding

                // Let's align center of window (width=100?) to x_global
                // Let's assume window width is ~100.
                let win_w = 100.0;
                let final_x = x_global - (win_w / 2.0);
                let final_y = y_global;

                eprintln!("[Caret] Moving window to: ({:.1}, {:.1})", final_x, final_y);
                let _ = window.set_position(tauri::LogicalPosition::new(final_x, final_y));
                return true;
            }
        } else {
            // eprintln!("[Caret] get_caret_rect returned None");
        }

        // Fallback to center bottom
        if let Ok(Some(monitor)) = window
            .current_monitor()
            .or_else(|_| window.primary_monitor())
        {
            let work_area = monitor.work_area();
            let scale_factor = monitor.scale_factor();

            // Convert physical pixels to logical points
            let work_area_width = work_area.size.width as f64 / scale_factor;
            let work_area_height = work_area.size.height as f64 / scale_factor;
            let work_area_x = work_area.position.x as f64 / scale_factor;
            let work_area_y = work_area.position.y as f64 / scale_factor;

            // Center horizontally: work_area_x + (work_area_width / 2) - (window_width / 2)
            let x = work_area_x + (work_area_width / 2.0) - 50.0; // 50 is half of 100

            // Position at bottom: work_area_y + work_area_height - window_height - 20px offset
            let y = work_area_y + work_area_height - 100.0 - 20.0;

            let _ = window.set_position(tauri::LogicalPosition::new(x, y));
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
            // Always show indicator; position to caret or screen center
            let found = update_position_to_caret(&w);
            let _ = w.show();
            if let Ok(ns_window) = w.ns_window() {
                unsafe {
                    let _: () = msg_send![ns_window as id, orderFront: nil];
                }
            }
            if found {
                eprintln!("[Indicator] Positioned at caret.");
            } else {
                eprintln!("[Indicator] Positioned at screen center (fallback).");
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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
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

            let icon = image::load_from_memory(include_bytes!("../icons/TrayIconTemplate@2x.png"))
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

            // Manage the menu state so we can access it elsewhere (e.g. save_config)
            app.manage(TrayMenu(menu.clone()));

            let _tray = TrayIconBuilder::with_id("tray")
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
                            let tray_menu = app.state::<TrayMenu>();
                            if let Some(MenuItemKind::Check(check_item)) =
                                tray_menu.0.get("show_main")
                            {
                                let _ = check_item.set_checked(!is_visible);
                            }
                        }
                    }
                    "toggle_dock" => {
                        let mut config = AppConfig::load_or_default().unwrap_or_default();
                        config.general.hide_dock_icon = !config.general.hide_dock_icon;
                        let _ = config.save();

                        let _ = config.save();

                        // Sync UI state
                        let tray_menu = app.state::<TrayMenu>();
                        if let Some(MenuItemKind::Check(check_item)) =
                            tray_menu.0.get("toggle_dock")
                        {
                            let _ = check_item.set_checked(config.general.hide_dock_icon);
                        }

                        #[cfg(target_os = "macos")]
                        {
                            macos_ext::set_dock_visible(!config.general.hide_dock_icon);
                            // Only restore settings window if it was already visible/open
                            if let Some(w) = app.get_webview_window("settings") {
                                if w.is_visible().unwrap_or(false) {
                                    let _ = w.set_focus();
                                }
                            }
                        }
                    }
                    "toggle_autostart" => {
                        let mut config = AppConfig::load_or_default().unwrap_or_default();
                        config.general.auto_start = !config.general.auto_start;
                        let _ = config.save();

                        // Sync Autostart
                        use tauri_plugin_autostart::ManagerExt;
                        let autostart_manager = app.autolaunch();
                        if config.general.auto_start {
                            let _ = autostart_manager.enable();
                        } else {
                            let _ = autostart_manager.disable();
                        }

                        // Sync UI state
                        let tray_menu = app.state::<TrayMenu>();
                        if let Some(MenuItemKind::Check(check_item)) =
                            tray_menu.0.get("toggle_autostart")
                        {
                            let _ = check_item.set_checked(config.general.auto_start);
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
        audio_capture.clone(),
        text_inserter,
    )));

    // 4. Hotkeys
    let hotkey_manager = Arc::new(HotkeyManager::new(&config.hotkey)?);

    // Set up hotkey callback
    let vc_clone = voice_controller.clone();
    let handle_clone = handle.clone();

    // Loop for monitoring focus change when recording
    let vc_monitor = voice_controller.clone();
    let handle_monitor = handle.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_pid = None;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            let is_recording = {
                let vc = vc_monitor.lock().await;
                vc.is_recording()
            };

            if is_recording {
                #[cfg(target_os = "macos")]
                {
                    if let Some(current_pid) = macos_ext::get_focused_pid() {
                        if let Some(last) = last_pid {
                            if current_pid != last {
                                eprintln!(
                                    "[AutoStop] Focus changed from {} to {}. Stopping.",
                                    last, current_pid
                                );
                                let mut vc = vc_monitor.lock().await;
                                if vc.is_recording() {
                                    let _ = vc.stop().await;

                                    // Play stop sound
                                    play_sound("/System/Library/Sounds/Pop.aiff");

                                    // Hide window
                                    if let Some(w) = handle_monitor.get_webview_window("main") {
                                        let _ = w.hide();
                                    }

                                    // Update frontend
                                    let _ = handle_monitor.emit(
                                        "asr-status",
                                        json!({
                                            "status": "idle",
                                            "text": ""
                                        }),
                                    );
                                }
                            }
                        }
                        last_pid = Some(current_pid);
                    }
                }
            } else {
                // Not recording, just update last_pid so we know where we started
                #[cfg(target_os = "macos")]
                {
                    if let Some(pid) = macos_ext::get_focused_pid() {
                        last_pid = Some(pid);
                    }
                }
            }
        }
    });

    // Loop for pushing volume to frontend
    let handle_vol = handle.clone();
    let audio_capture_vol = audio_capture.clone();
    let vc_vol = voice_controller.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            let is_recording = {
                if let Ok(vc) = vc_vol.try_lock() {
                    vc.is_recording()
                } else {
                    false
                }
            };

            if is_recording {
                let volume = audio_capture_vol.get_volume();
                let _ = handle_vol.emit("asr-volume", json!({ "volume": volume }));
            }
        }
    });

    // Loop for updating indicator position while recording (Cursor Following)
    let vc_caret = voice_controller.clone();
    let handle_caret = handle.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            let is_recording = {
                if let Ok(vc) = vc_caret.try_lock() {
                    vc.is_recording()
                } else {
                    false
                }
            };

            if is_recording {
                #[cfg(target_os = "macos")]
                {
                    if let Some(w) = handle_caret.get_webview_window("main") {
                        let w_clone = w.clone();
                        let _ = w.run_on_main_thread(move || {
                            // Only update if we can find the caret, otherwise keep existing position?
                            // Or continuously fallback? The update_position_to_caret logic handles fallback.
                            // But we probably don't want to reset to bottom-center if we lose caret for a split second
                            // while typing...
                            // For now, let's just call it. It tries to be smart.
                            macos_ext::update_position_to_caret(&w_clone);
                        });
                    }
                }
            }
        }
    });

    let callback: Arc<dyn Fn() + Send + Sync + 'static> = Arc::new(move || {
        let vc = vc_clone.clone();
        let h = handle_clone.clone();
        tauri::async_runtime::spawn(async move {
            let mut vc_lock = vc.lock().await;

            // Check if we are currently recording. If not, we need to check for caret position first.
            if !vc_lock.is_recording() {
                #[cfg(target_os = "macos")]
                {
                    // Check accessibility permission first
                    if !macos_ext::is_accessibility_enabled() {
                        eprintln!("Accessibility permission is NOT enabled.");
                        // We still try to get caret, but it will likely fail.
                        // Or we can choose to explicitly warn the user here if we had a way (e.g. emit event).
                    }

                    // Attempt to get caret position for indicator follow
                    if macos_ext::get_indicator_position().is_none() {
                        eprintln!("No caret position detected (Accessibility enabled: {}). Proceeding anyway.", macos_ext::is_accessibility_enabled());
                        // NOT returning here; allowing speech input to proceed.
                    }
                }
            }

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
