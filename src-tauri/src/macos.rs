use objc2::MainThreadMarker;
use objc2_app_kit::{NSWindow, NSWindowButton, NSWindowStyleMask};
use tauri::{WebviewWindow, Window, WindowEvent};

// Match WindowBar's h-8; the frontend title centers independently of the controls.
const TITLEBAR_HEIGHT: f64 = 32.0;
const LEFT_INSET: f64 = 14.0;

pub fn initialize(window: &WebviewWindow) -> tauri::Result<()> {
    center_traffic_lights(window.ns_window()?);
    Ok(())
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    // Tao also emits Resized after leaving fullscreen, once AppKit has restored
    // the normal titlebar. Keep the system-managed fullscreen controls intact.
    if window.label() == "main"
        && matches!(
            event,
            WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(true)
                | WindowEvent::ThemeChanged(_)
        )
        && let Ok(handle) = window.ns_window()
    {
        center_traffic_lights(handle);
    }
}

fn center_traffic_lights(handle: *mut std::ffi::c_void) {
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: Tauri supplies this live NSWindow handle, and AppKit access is
    // confined to the main thread while the owning Tauri window is alive.
    let Some(window) = (unsafe { handle.cast::<NSWindow>().as_ref() }) else {
        return;
    };
    if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return;
    }
    let (Some(close), Some(minimize), Some(zoom)) = (
        window.standardWindowButton(NSWindowButton::CloseButton),
        window.standardWindowButton(NSWindowButton::MiniaturizeButton),
        window.standardWindowButton(NSWindowButton::ZoomButton),
    ) else {
        return;
    };
    // SAFETY: The live window owns these AppKit views; their hierarchy is read
    // on the main thread and the returned parent views are retained.
    let Some(container) = (unsafe { close.superview().and_then(|parent| parent.superview()) })
    else {
        return;
    };
    let spacing = minimize.frame().origin.x - close.frame().origin.x;
    let window_height = window.frame().size.height;
    let mut frame = container.frame();
    frame.size.height = TITLEBAR_HEIGHT;
    frame.origin.y = window_height - TITLEBAR_HEIGHT;
    container.setFrame(frame);
    container.layoutSubtreeIfNeeded();

    for (index, button) in [close, minimize, zoom].into_iter().enumerate() {
        // SAFETY: The button remains owned by the same live window above.
        let Some(parent) = (unsafe { button.superview() }) else {
            continue;
        };
        let frame = button.frame();
        // Convert the desired window-space center into the button's own parent
        // coordinates; SDK-dependent heights and native Y offsets cancel out.
        let mut center = frame.origin;
        center.x = LEFT_INSET + index as f64 * spacing + frame.size.width / 2.0;
        center.y = window_height - TITLEBAR_HEIGHT / 2.0;
        let center = parent.convertPoint_fromView(center, None);
        let mut origin = frame.origin;
        origin.x = center.x - frame.size.width / 2.0;
        origin.y = center.y - frame.size.height / 2.0;
        button.setFrameOrigin(origin);

        #[cfg(debug_assertions)]
        if std::env::var_os("KVAULT_TITLEBAR_DIAGNOSTICS").is_some() {
            let rect = button.convertRect_toView(button.bounds(), None);
            eprintln!(
                "titlebar button={index} height={} center_from_top={} left={}",
                frame.size.height,
                window_height - rect.origin.y - rect.size.height / 2.0,
                rect.origin.x,
            );
        }
    }
}
