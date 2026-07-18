//! 菜单栏图标：从内嵌 PNG 加载。
//!
//! 三种状态,想换图标只要覆盖对应 PNG 再重新编译,不用改这里的代码:
//! - `icons/tray-idle.png`      —— 空闲、daemon 已连接:`>_` 实心下划线
//! - `icons/tray-offline.png`   —— 空闲、daemon 未连接:`>_` 空心描边下划线
//! - `icons/tray-busy-0..3.png` —— 忙:`>` + 块光标,4 帧上下摆动,逐帧循环成动画
//!
//! 这些都是纯黑 + alpha 的单色 template 图:托盘用 `icon_as_template(true)`(见
//! lib.rs),系统会按菜单栏前景色自动上黑/白。要按原色显示改回 `false`。
//!
//! 图标由 `scratchpad/gen_tray.py`(PIL)生成,想调大小/粗细/摆幅改那个脚本重跑。

use tauri::image::Image;

/// 空闲、daemon 已连接。
static IDLE_PNG: &[u8] = include_bytes!("../icons/tray-idle.png");
/// 空闲、daemon 未连接(离线)。
static OFFLINE_PNG: &[u8] = include_bytes!("../icons/tray-offline.png");
/// 忙:块光标上下摆动的各帧,按顺序循环播放。
static BUSY_FRAMES: [&[u8]; 4] = [
    include_bytes!("../icons/tray-busy-0.png"),
    include_bytes!("../icons/tray-busy-1.png"),
    include_bytes!("../icons/tray-busy-2.png"),
    include_bytes!("../icons/tray-busy-3.png"),
];

/// 忙状态动画的总帧数。
pub const BUSY_FRAME_COUNT: usize = BUSY_FRAMES.len();

/// 图片是编译期固定的,解码不该失败;万一 PNG 损坏,直接 panic 好过悄悄显示不出图标。
fn decode(bytes: &'static [u8]) -> Image<'static> {
    Image::from_bytes(bytes).expect("内置托盘 PNG 解码失败")
}

/// 空闲、daemon 已连接的图标。
pub fn idle() -> Image<'static> {
    decode(IDLE_PNG)
}

/// 空闲、daemon 未连接的图标。
pub fn offline() -> Image<'static> {
    decode(OFFLINE_PNG)
}

/// 忙状态第 `frame` 帧(自动按帧数取模,调用方不用担心越界)。
pub fn busy(frame: usize) -> Image<'static> {
    decode(BUSY_FRAMES[frame % BUSY_FRAME_COUNT])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_icons_decode() {
        // 所有内置 PNG 都要能解码,否则运行时会 panic。
        let _ = idle();
        let _ = offline();
        for i in 0..BUSY_FRAME_COUNT {
            let _ = busy(i);
        }
    }
}
