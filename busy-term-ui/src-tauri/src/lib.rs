mod config;
mod daemon;
mod events;
mod icon;

use std::time::Duration;

use config::Whitelist;
use daemon::DaemonManager;
use events::CommandTracker;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

/// 设置面板（独立窗口，齿轮点开）。
#[tauri::command]
fn open_settings(app: tauri::AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn show_settings(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 把 popover 摆到菜单栏图标正下方、水平居中对齐。
fn position_popover(window: &tauri::WebviewWindow, rect: tauri::Rect) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let icon_pos = rect.position.to_physical::<f64>(scale);
    let icon_size = rect.size.to_physical::<f64>(scale);
    let Ok(win_size) = window.outer_size() else {
        return;
    };
    let x = icon_pos.x + icon_size.width / 2.0 - win_size.width as f64 / 2.0;
    let y = icon_pos.y + icon_size.height;
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// 按「当前有没有在显示的命令」切菜单栏图标。
///
/// 必须轮询而不是只在收到事件时更新：命令跨过 3 秒门槛这件事本身不产生任何事件，
/// 一条命令是「跑着跑着」才变成该显示的。
fn spawn_icon_updater(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut shown: Option<bool> = None;
        loop {
            let busy = !events::visible_commands(&app.state::<CommandTracker>(), &app.state::<Whitelist>())
                .is_empty();
            if shown != Some(busy) {
                if let Some(tray) = app.tray_by_id("main-tray") {
                    let _ = tray.set_icon(Some(icon::image(busy)));
                }
                shown = Some(busy);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

fn toggle_popover(app: &tauri::AppHandle, rect: tauri::Rect) {
    let Some(window) = app.get_webview_window("popover") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    position_popover(&window, rect);
    let _ = window.show();
    let _ = window.set_focus();
    // webview 是常驻的，只是窗口在隐藏/显示，CSS 动画不会自己重播。
    // 每次显示都发个事件，让前端把入场动画重新跑一遍。
    let _ = window.emit("popover-shown", ());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(DaemonManager::default())
        .manage(CommandTracker::default())
        .invoke_handler(tauri::generate_handler![
            daemon::daemon_status,
            daemon::start_daemon,
            daemon::restart_daemon,
            daemon::stop_daemon,
            events::running_commands,
            config::whitelist_get,
            config::whitelist_add,
            config::whitelist_remove,
            open_settings,
            quit_app,
        ])
        .setup(|app| {
            // 菜单栏应用：不在 Dock 里占位，也不抢 App Switcher。
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let whitelist_path = app
                .path()
                .app_config_dir()
                .map_err(|err| format!("拿不到配置目录：{err}"))?
                .join("whitelist.json");
            app.manage(Whitelist::load(whitelist_path));

            events::spawn_reader(app.handle().clone());

            let settings_item = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 BusyTerm", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;

            TrayIconBuilder::with_id("main-tray")
                .icon(icon::image(false))
                // 单色 template 图：随菜单栏明暗自动黑/白。想按 PNG 原色显示改回 false。
                .icon_as_template(true)
                .tooltip("BusyTerm")
                .menu(&menu)
                // 左键归 popover，右键才出菜单。
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => show_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_popover(tray.app_handle(), rect);
                    }
                })
                .build(app)?;

            // 必须放在 tray 建好、Whitelist manage 之后：它两个都要用。
            spawn_icon_updater(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // 关设置窗口只是收起来，不退出 app，也不动 daemon。
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // popover 点到别处就收起，符合菜单栏面板的习惯。
            WindowEvent::Focused(false) if window.label() == "popover" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // app 独占 daemon：退出时一并收掉，别留孤儿进程。
            if let tauri::RunEvent::Exit = event {
                let _ = app.state::<DaemonManager>().stop();
            }
        });
}
