//! 管理 busy-term-daemon 子进程：启动、停止、重启、查状态。
//!
//! app 独占 daemon：只认自己 spawn 出来的子进程，退出时一并收掉。
//! 手动在 iTerm2 里跑起来的 daemon 不会被识别成「已启动」。

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;

/// 找 daemon 可执行文件（PyInstaller 冻出来的单文件，自带 Python 运行时）。
///
/// 分发后 GUI app 的 PATH 只有 /usr/bin:/bin:/usr/sbin:/sbin，也没有 uv/python，
/// 所以只认三个来源，优先级从高到低：
///   1. BUSY_TERM_DAEMON_BIN —— 显式覆盖，调试/测试用。
///   2. 打包资源 —— .app/Contents/MacOS/<exe> 旁边的 ../Resources/busy-term-daemon。
///      用 current_exe 反推，避免把 AppHandle 一路传进来。
///   3. 开发期回退 —— 源码树里 build.sh 冻出来的 busy-term-daemon/dist/busy-term-daemon。
fn daemon_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("BUSY_TERM_DAEMON_BIN") {
        return Some(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        // exe = .../Contents/MacOS/busy-term-ui → .../Contents/Resources/busy-term-daemon
        let bundled = exe
            .parent()
            .and_then(|macos| macos.parent())
            .map(|contents| contents.join("Resources/busy-term-daemon"));
        if let Some(p) = bundled.filter(|p| p.is_file()) {
            return Some(p);
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("busy-term-daemon/dist/busy-term-daemon"))
        .filter(|p| p.is_file())
}

/// daemon 的单一状态。
///
/// 这里必须把两个事实合起来看，它们会不一致：
///   owned   = 本 app spawn 的子进程还活着
///   serving = socket 连得上（daemon 真的在工作）
/// 典型的不一致：上一个 app 实例被强杀后留下孤儿 daemon —— 它还在服务
/// （serving），但不是当前 app 的孩子（!owned）。之前两个面板各读一个信号，
/// 于是一个显示「运行中」、一个显示「未启动」。
#[derive(Serialize, Clone)]
pub struct DaemonStatus {
    /// stopped | starting | running | external
    pub state: &'static str,
    pub pid: Option<u32>,
    /// 上一次启动失败的原因，用于在面板上直说哪里不对，而不是只显示「未启动」。
    pub last_error: Option<String>,
}

fn classify(owned: bool, serving: bool) -> &'static str {
    match (owned, serving) {
        (true, true) => "running",
        // 进程刚起来、socket 还没 bind 好；如果一直停在这个状态，
        // 多半是 iTerm2 没授权或没运行，daemon 起来了却连不上。
        (true, false) => "starting",
        // 不是我们的孩子，却在服务 —— 孤儿 daemon，或者用户手动跑的。
        (false, true) => "external",
        (false, false) => "stopped",
    }
}

#[derive(Default)]
pub struct DaemonManager {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    child: Option<Child>,
    last_error: Option<String>,
}

impl DaemonManager {
    /// 回收已经退出的子进程，避免留下僵尸进程，也让状态如实反映现实。
    fn reap(inner: &mut Inner) {
        let exited = match inner.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => {
                    inner.last_error = Some(format!("daemon 已退出（{status}）"));
                    true
                }
                Ok(None) => false,
                Err(err) => {
                    inner.last_error = Some(format!("无法查询 daemon 状态：{err}"));
                    true
                }
            },
            None => false,
        };
        if exited {
            inner.child = None;
        }
    }

    /// 只回答「我们的子进程活着吗」；要拿给界面看的状态请走 daemon_status 命令。
    fn owned(&self) -> (bool, Option<u32>, Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        Self::reap(&mut inner);
        (
            inner.child.is_some(),
            inner.child.as_ref().map(|c| c.id()),
            inner.last_error.clone(),
        )
    }

    pub fn start(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        Self::reap(&mut inner);
        if inner.child.is_some() {
            return Err("daemon 已经在运行了".into());
        }

        let bin = daemon_bin().ok_or_else(|| {
            let msg = "找不到 daemon 可执行文件（打包资源和 dist/ 里都没有，先跑 build.sh 冻一个）"
                .to_string();
            inner.last_error = Some(msg.clone());
            msg
        })?;
        // Tauri 打包 resource 时可能丢掉可执行位，spawn 前补回来。
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&bin) {
                let mode = meta.permissions().mode();
                if mode & 0o111 == 0 {
                    let mut perms = meta.permissions();
                    perms.set_mode(mode | 0o755);
                    let _ = std::fs::set_permissions(&bin, perms);
                }
            }
        }

        let mut command = Command::new(&bin);
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            // 让 daemon 自成一个 session/进程组。必须这么做：否则子进程会继承
            // app 自己的进程组，后面 stop() 里的 kill(-pid) 会把 app 一起干掉。
            // 顺带也让 daemon 不再继承终端的信号。
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|err| {
                let msg = format!("启动 daemon 失败：{err}");
                inner.last_error = Some(msg.clone());
                msg
            })?;

        inner.child = Some(child);
        inner.last_error = None;
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(mut child) = inner.child.take() {
            // daemon 可能自己再 fork 子进程，直接对整个进程组发信号，一锅端。
            kill_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.last_error = None;
        Ok(())
    }

    pub fn restart(&self) -> Result<(), String> {
        self.stop()?;
        self.start()
    }
}

/// 给进程组发 SIGTERM。daemon 收到后会走正常退出路径，清理 socket 文件。
fn kill_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    /// 真的把 daemon 拉起来，确认它连上了 iTerm2 并把 socket 开出来了，
    /// 然后确认 stop() 能把整个进程组收干净。
    /// 需要 iTerm2 正在运行。
    #[test]
    fn start_then_stop_leaves_nothing_behind() {
        let manager = DaemonManager::default();

        manager.start().expect("start 失败");
        let (owned, pid, _) = manager.owned();
        assert!(owned);
        let pid = pid.expect("应该有 pid");

        // 等 socket 出现，说明 daemon 认证通过并且服务端起来了。
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut connected = false;
        while Instant::now() < deadline {
            if UnixStream::connect("/tmp/busy-term.sock").is_ok() {
                connected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        assert!(connected, "20s 内没能连上 daemon 的 socket");
        assert!(manager.owned().0);

        manager.stop().expect("stop 失败");
        assert!(!manager.owned().0);

        // 进程组真的没了吗？kill(pid, 0) 应该失败。
        std::thread::sleep(Duration::from_millis(800));
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        assert!(!alive, "stop() 之后 daemon 进程 {pid} 还活着");
        // 查文件本身，不是查「连不连得上」——残留的死 socket 文件同样连不上，
        // 用连通性判断会把「文件泄漏」当成通过。
        assert!(
            !Path::new("/tmp/busy-term.sock").exists(),
            "daemon 停了，socket 文件却还在（SIGTERM 没走干净的退出路径？）"
        );
    }
}

/// 界面唯一的状态来源。popover 和设置面板都读它，所以两边不可能再各说各话。
#[tauri::command]
pub fn daemon_status(
    manager: tauri::State<'_, DaemonManager>,
    tracker: tauri::State<'_, crate::events::CommandTracker>,
) -> DaemonStatus {
    let (owned, pid, last_error) = manager.owned();
    DaemonStatus {
        state: classify(owned, tracker.connected()),
        pid,
        last_error,
    }
}

#[tauri::command]
pub fn start_daemon(manager: tauri::State<'_, DaemonManager>) -> Result<(), String> {
    manager.start()
}

#[tauri::command]
pub fn restart_daemon(manager: tauri::State<'_, DaemonManager>) -> Result<(), String> {
    manager.restart()
}

#[tauri::command]
pub fn stop_daemon(manager: tauri::State<'_, DaemonManager>) -> Result<(), String> {
    manager.stop()
}
