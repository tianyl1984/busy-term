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

/// 菜单栏图标的三种状态。空闲态再按 socket 连通性分「已连接 / 离线」。
#[derive(Clone, Copy, PartialEq)]
enum IconState {
    /// 有命令在跑：块光标上下摆动的动画。
    Busy,
    /// 空闲，且 daemon socket 连得上。
    Idle,
    /// 空闲，且 daemon 未启动 / 连不上。
    Offline,
}

/// 具体要贴的那一张图（忙态还带帧号）。用它把「贴哪张」的决定从后台线程
/// 传进主线程闭包里再解码，避免把 `Image` 跨线程搬运。
#[derive(Clone, Copy)]
enum IconFrame {
    Busy(usize),
    Idle,
    Offline,
}

/// 每秒刷新一次菜单栏图标。
///
/// 必须轮询而不是只在收到事件时更新：一是命令跨过 3 秒门槛这件事本身不产生任何事件，
/// 一条命令是「跑着跑着」才变成该显示的；二是忙状态的光标动画本来就要按固定节拍走帧。
/// 空闲/离线态图标不变时不重贴，只有忙态每秒换一帧做「上下摆」动画。
fn spawn_icon_updater(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut shown: Option<IconState> = None;
        let mut frame = 0usize;
        loop {
            let busy = !events::visible_commands(
                &app.state::<CommandTracker>(),
                &app.state::<Whitelist>(),
            )
            .is_empty();
            let state = if busy {
                IconState::Busy
            } else if app.state::<CommandTracker>().connected() {
                IconState::Idle
            } else {
                IconState::Offline
            };

            // 决定这一轮要贴哪张图：忙态每帧都换（动画）；空闲/离线态只在切换时贴一次。
            let which = match state {
                IconState::Busy => {
                    let f = frame;
                    frame = (frame + 1) % icon::BUSY_FRAME_COUNT;
                    Some(IconFrame::Busy(f))
                }
                IconState::Idle if shown != Some(state) => {
                    frame = 0;
                    Some(IconFrame::Idle)
                }
                IconState::Offline if shown != Some(state) => {
                    frame = 0;
                    Some(IconFrame::Offline)
                }
                _ => None,
            };
            shown = Some(state);

            if let Some(which) = which {
                let handle = app.clone();
                // set_icon 与 set_icon_as_template 必须在同一个主线程闭包里连着做完：
                // 分两次从后台线程分派到主线程的话，中间会先画出一帧「非 template」的黑图，
                // 整个图标（包括左边的 >）就会每秒闪一下。合成一次主线程操作即可消除闪动。
                let _ = app.run_on_main_thread(move || {
                    if let Some(tray) = handle.tray_by_id("main-tray") {
                        let image = match which {
                            IconFrame::Busy(f) => icon::busy(f),
                            IconFrame::Idle => icon::idle(),
                            IconFrame::Offline => icon::offline(),
                        };
                        let _ = tray.set_icon(Some(image));
                        // set_icon 会重置 template 标志，不重设图标会变黑、不再随菜单栏明暗上白色。
                        let _ = tray.set_icon_as_template(true);
                    }
                });
            }
            // 换帧节拍 = 图标「每秒换一个位置」，也决定忙/闲切换的最大延迟。
            std::thread::sleep(Duration::from_secs(1));
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
                // 启动瞬间还没连上 daemon，先按离线态显示，连上后 updater 会改。
                .icon(icon::offline())
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
