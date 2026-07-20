//! 白名单：命中的命令不在 popover 里显示。
//!
//! 只影响 UI 显示——daemon 照常广播全部事件，socket 对其他客户端保持完整。
//! 所以改白名单立即生效，不用重启 daemon。

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// 取命令的程序名：第一个词，去掉路径。
///
/// `/opt/homebrew/bin/uv run main.py` → `uv`
pub fn program_name(command: &str) -> &str {
    let first = command.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first)
}

pub struct Whitelist {
    entries: Mutex<Vec<String>>,
    path: PathBuf,
}

impl Whitelist {
    /// 读不出来（首次运行、文件损坏）就当空白名单，不该因为这个拦住 app 启动。
    pub fn load(path: PathBuf) -> Self {
        let entries = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();
        Self {
            entries: Mutex::new(entries),
            path,
        }
    }

    fn persist(&self, entries: &[String]) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|err| format!("创建配置目录失败：{err}"))?;
        }
        let raw = serde_json::to_string_pretty(entries)
            .map_err(|err| format!("序列化白名单失败：{err}"))?;
        fs::write(&self.path, raw).map_err(|err| format!("写入白名单失败：{err}"))
    }

    pub fn list(&self) -> Vec<String> {
        self.entries.lock().unwrap().clone()
    }

    pub fn add(&self, program: String) -> Result<Vec<String>, String> {
        // 存归一化后的程序名：用户手输 `./dev.sh`，hides() 那边算出来的是 `dev.sh`，
        // 不在这里对齐就永远匹配不上。
        let program = program_name(program.trim()).to_string();
        if program.is_empty() {
            return Err("白名单项不能为空".into());
        }
        let mut entries = self.entries.lock().unwrap();
        if entries.iter().any(|e| e == &program) {
            return Ok(entries.clone());
        }
        entries.push(program);
        entries.sort();
        self.persist(&entries)?;
        Ok(entries.clone())
    }

    pub fn remove(&self, program: &str) -> Result<Vec<String>, String> {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|e| e != program);
        self.persist(&entries)?;
        Ok(entries.clone())
    }

    /// 这条命令要不要藏起来。
    pub fn hides(&self, command: &str) -> bool {
        let program = program_name(command);
        // entry 也过一遍：老配置文件里可能存着未归一化的 `./dev.sh`。
        self.entries
            .lock()
            .unwrap()
            .iter()
            .any(|e| program_name(e) == program)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_name_strips_args_and_path() {
        assert_eq!(program_name("npm run build"), "npm");
        assert_eq!(program_name("/opt/homebrew/bin/uv run main.py"), "uv");
        assert_eq!(program_name("ls"), "ls");
        assert_eq!(program_name(""), "");
        // 首词匹配的已知边界：环境变量前缀会被当成程序名。
        assert_eq!(program_name("FOO=1 npm test"), "FOO=1");
    }

    #[test]
    fn hides_matches_on_program_only() {
        let dir = std::env::temp_dir().join(format!("busy-term-test-{}", std::process::id()));
        let list = Whitelist::load(dir.join("whitelist.json"));
        list.add("npm".into()).unwrap();

        assert!(list.hides("npm run build"));
        assert!(list.hides("npm"));
        // 只看程序名：参数里出现 npm 不该被藏。
        assert!(!list.hides("echo npm"));
        assert!(!list.hides("npmx run"));

        list.remove("npm").unwrap();
        assert!(!list.hides("npm run build"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_normalizes_path_prefixed_input() {
        let dir = std::env::temp_dir().join(format!("busy-term-test-norm-{}", std::process::id()));
        let list = Whitelist::load(dir.join("whitelist.json"));

        // 用户在设置里手输带路径的写法，也要能挡住。
        list.add("./dev.sh".into()).unwrap();
        assert_eq!(list.list(), vec!["dev.sh".to_string()]);
        assert!(list.hides("./dev.sh"));
        assert!(list.hides("dev.sh --port 3000"));
        assert!(list.hides("/Users/me/proj/dev.sh"));

        let _ = fs::remove_dir_all(&dir);
    }
}

#[tauri::command]
pub fn whitelist_get(list: tauri::State<'_, Whitelist>) -> Vec<String> {
    list.list()
}

#[tauri::command]
pub fn whitelist_add(
    list: tauri::State<'_, Whitelist>,
    program: String,
) -> Result<Vec<String>, String> {
    list.add(program)
}

#[tauri::command]
pub fn whitelist_remove(
    list: tauri::State<'_, Whitelist>,
    program: String,
) -> Result<Vec<String>, String> {
    list.remove(&program)
}
