//! 菜单栏图标：从内嵌 PNG 加载。
//!
//! 忙 / 闲各一张 PNG，编译期用 `include_bytes!` 打进二进制。想换图标只要覆盖
//! `icons/tray-busy.png`（有命令在跑时显示，实心块光标）和
//! `icons/tray-idle.png`（空闲时显示，细下划线光标）这两个文件，再重新编译即可，
//! 不用改这里的代码。
//!
//! 这两张是纯黑 + alpha 的单色 template 图：托盘用 `icon_as_template(true)`（见
//! lib.rs），系统会按菜单栏前景色自动上黑/白。要换成按原色显示的彩色图，把那里
//! 改回 `false` 即可。

use tauri::image::Image;

/// 有命令在跑时显示的图标。
static BUSY_PNG: &[u8] = include_bytes!("../icons/tray-busy.png");
/// 空闲时显示的图标。
static IDLE_PNG: &[u8] = include_bytes!("../icons/tray-idle.png");

/// 按忙/闲返回对应的托盘图标。
///
/// 图片是编译期固定的，解码不该失败；万一 PNG 损坏，直接 panic 好过悄悄显示不出图标。
pub fn image(busy: bool) -> Image<'static> {
    let bytes = if busy { BUSY_PNG } else { IDLE_PNG };
    Image::from_bytes(bytes).expect("内置托盘 PNG 解码失败")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_icons_decode() {
        // 两张内置 PNG 都要能解码，否则运行时会 panic。
        let _ = image(true);
        let _ = image(false);
    }
}
