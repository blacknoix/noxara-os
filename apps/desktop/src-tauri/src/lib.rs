//! CompanyOS Tauri desktop shell — wraps `apps/web`.

mod commands;

use companyos_desktop_shell::{default_web_url, COPILOT_HOTKEY};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_web_url,
            commands::load_cached_dashboard,
            commands::save_cached_dashboard,
            commands::parse_deep_link,
            commands::open_deep_link,
            commands::copilot_hotkey,
        ])
        .setup(|app| {
            // System tray
            let show = MenuItem::with_id(app, "show", "Show CompanyOS", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("CompanyOS")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Global copilot hotkey Alt+Space / ⌥Space
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
                let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::Space);
                let handle = app.handle().clone();
                app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, _event| {
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.eval(
                            "window.dispatchEvent(new CustomEvent('companyos:copilot-hotkey'))",
                        );
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                })?;
                let _ = COPILOT_HOTKEY;
            }

            // Deep links → navigate wrapped web app
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    let urls = event.urls();
                    for url in urls {
                        if let Ok(nav) = commands::resolve_url(url.as_str(), None) {
                            if let Some(w) = handle.get_webview_window("main") {
                                let path = nav.web_path;
                                let org = nav.org_id;
                                let js = format!(
                                    "window.dispatchEvent(new CustomEvent('companyos:deep-link', {{ detail: {{ path: '{path}', org: '{org}' }} }}));"
                                );
                                let _ = w.eval(&js);
                            }
                        }
                    }
                })?;
            }

            let _ = default_web_url();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Close to tray instead of quitting.
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running CompanyOS desktop");
}
