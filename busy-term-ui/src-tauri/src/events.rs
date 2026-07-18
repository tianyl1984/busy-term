//! 连接 daemon 的 unix socket，跟踪「当前正在执行的命令」。
//!
//! daemon 发的是 NDJSON 事件流（command_start / command_end），本身不保存状态，
//! 也没有重放：谁连上才收得到。所以这里维护的表只反映「连上之后」发生的事——
//! daemon 重启或 UI 后连，之前就已经在跑的命令是看不见的。

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::Manager;

pub fn socket_path() -> PathBuf {
    std::env::var_os("BUSY_TERM_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/busy-term.sock"))
}

#[derive(Clone, Serialize)]
pub struct RunningCommand {
    pub session_id: String,
    pub command: String,
    /// daemon 给的 epoch 秒；前端用它算已运行时长。
    pub started_at: f64,
}


#[derive(Default)]
pub struct CommandTracker {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    connected: bool,
    // 一个 iTerm2 会话同一时刻只会有一条前台命令，用 session_id 做键就够。
    running: HashMap<String, RunningCommand>,
}

impl CommandTracker {
    fn set_connected(&self, connected: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.connected = connected;
        if !connected {
            // 断开后旧数据全是猜测，不如清掉——宁可显示空，也不显示假的「还在跑」。
            inner.running.clear();
        }
    }

    fn apply(&self, line: &str) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        let Some(session_id) = event.get("session_id").and_then(|v| v.as_str()) else {
            return;
        };
        let mut inner = self.inner.lock().unwrap();
        match event.get("type").and_then(|v| v.as_str()) {
            Some("command_start") => {
                let command = event
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let started_at = event.get("ts").and_then(|v| v.as_f64()).unwrap_or_default();
                inner.running.insert(
                    session_id.to_string(),
                    RunningCommand {
                        session_id: session_id.to_string(),
                        command,
                        started_at,
                    },
                );
            }
            Some("command_end") => {
                // 新开的会话在第一个 prompt 出现时会补报一条 command_end，
                // 这时表里本来就没有对应项，remove 是空操作，正好忽略掉。
                inner.running.remove(session_id);
            }
            _ => {}
        }
    }

    /// socket 是否连得上。这是「daemon 真的在工作」的判据。
    pub fn connected(&self) -> bool {
        self.inner.lock().unwrap().connected
    }

    pub fn commands(&self) -> Vec<RunningCommand> {
        let inner = self.inner.lock().unwrap();
        let mut commands: Vec<RunningCommand> = inner.running.values().cloned().collect();
        // 跑得最久的排前面——面板是用来看「什么卡住了」的。
        commands.sort_by(|a, b| a.started_at.total_cmp(&b.started_at));
        commands
    }
}

/// 后台线程：连 socket、读事件、断了就重连。
///
/// daemon 可能还没启动、也可能被重启，所以这里必须是「一直重试」而不是连一次就算。
pub fn spawn_reader(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        if let Ok(stream) = UnixStream::connect(socket_path()) {
            app.state::<CommandTracker>().set_connected(true);
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                match line {
                    Ok(line) => app.state::<CommandTracker>().apply(&line),
                    Err(_) => break,
                }
            }
            // 读到 EOF 或出错 = daemon 没了。
            app.state::<CommandTracker>().set_connected(false);
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}

/// 跑够这么久才显示。绝大多数命令几十毫秒就结束了，全都显示的话面板会疯狂闪。
/// 面板要回答的是「什么在拖时间」，短命令本来也不值得看。
const MIN_VISIBLE_SECS: f64 = 3.0;

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

/// 「当前该显示哪些命令」的唯一判断处：白名单 + 3 秒门槛。
///
/// popover 和菜单栏图标都走这里。图标要是自己另算一套，就会出现
/// 「图标显示忙、面板里却是空的」这种自相矛盾。
pub fn visible_commands(
    tracker: &CommandTracker,
    whitelist: &crate::config::Whitelist,
) -> Vec<RunningCommand> {
    let now = now_epoch();
    tracker
        .commands()
        .into_iter()
        .filter(|cmd| now - cmd.started_at >= MIN_VISIBLE_SECS)
        .filter(|cmd| !whitelist.hides(&cmd.command))
        .collect()
}

#[tauri::command]
pub fn running_commands(
    tracker: tauri::State<'_, CommandTracker>,
    whitelist: tauri::State<'_, crate::config::Whitelist>,
) -> Vec<RunningCommand> {
    visible_commands(&tracker, &whitelist)
}
