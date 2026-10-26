//! Tray and window lifecycle. Closing is resolved by the UI so unsaved drafts
//! receive the same protection for native buttons, tray menus and system quit.
use tauri::{
    AppHandle, Emitter, Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn request(app: &AppHandle, exit: bool) {
    show_main(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(
            if exit {
                "app:exit-requested"
            } else {
                "app:close-requested"
            },
            (),
        );
    }
}

pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() == "main"
        && let WindowEvent::CloseRequested { api, .. } = event
    {
        api.prevent_close();
        request(window.app_handle(), false);
    }
}

#[cfg(target_os = "macos")]
fn template_icon() -> tauri::image::Image<'static> {
    // A transparent monochrome lock; macOS applies the correct menu-bar color.
    let mut rgba = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let radius = ((x as f32 - 15.5).powi(2) + (y as f32 - 11.5).powi(2)).sqrt();
            let shackle = (5..=12).contains(&y) && (5.0..=7.5).contains(&radius);
            let sides = (12..=16).contains(&y) && ((8..=10).contains(&x) || (21..=23).contains(&x));
            let body = (6..=25).contains(&x)
                && (14..=27).contains(&y)
                && (!(9..=22).contains(&x) || !(17..=24).contains(&y));
            let keyhole = (15..=16).contains(&x) && (19..=22).contains(&y);
            if shackle || sides || body || keyhole {
                rgba[(y * 32 + x) * 4 + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 32, 32)
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开 KVault", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 KVault", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("KVault")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => request(app, true),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon(template_icon()).icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
