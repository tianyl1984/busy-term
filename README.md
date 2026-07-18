# BusyTerm

一个 macOS 菜单栏小工具,盯着你的 **iTerm2** 会话,把「哪些命令正在跑、跑了多久」直接摆在菜单栏和一个下拉面板里。适合用来一眼看出「哪个终端还在忙 / 卡住了」。

> 仅支持 macOS + iTerm2。

## 功能

- **菜单栏图标**随状态变化(单色图标,跟随菜单栏明暗自动黑/白):

  | 状态 | 图标 | 含义 |
  |---|---|---|
  | 忙 | `❯` + 块光标(上下摆动动画) | 有命令已经跑了 ≥3 秒 |
  | 空闲 · 已连接 | `❯_` 实心下划线 | 没有在跑的命令,daemon 正常工作 |
  | 空闲 · 离线 | `❯▭` 空心下划线 | daemon 未启动 / 连不上 |

- **左键点图标**弹出面板,列出当前正在执行的命令,按已运行时长排序(跑得最久的在最上)。
- **白名单**:把不想显示的命令(如 `vim`、`ssh`)加进去,面板和图标都会忽略它。
- **右键点图标**出菜单:打开设置、退出。
- 菜单栏应用,不占 Dock、不抢 App Switcher。

## 架构

两个子项目,中间用一条 unix socket 打通:

```
┌─────────────────────┐   NDJSON over        ┌──────────────────────┐
│  busy-term-daemon    │   /tmp/busy-term.sock │  busy-term-ui        │
│  (Python 3.14)       │ ───────────────────▶ │  (Tauri + Vue 3)     │
│  订阅 iTerm2 Python   │   command_start /     │  订阅 socket、显示    │
│  API,广播命令事件     │   command_end 事件     │  菜单栏图标 + 面板    │
└─────────────────────┘                       └──────────────────────┘
```

- **`busy-term-daemon/`** —— Python 守护进程。通过 iTerm2 的本地 Python API 订阅各会话的命令开始/结束事件,以 NDJSON 广播到 unix socket(默认 `/tmp/busy-term.sock`)。广播是发了就忘,没有重放或缓冲。
- **`busy-term-ui/`** —— Tauri 2 + Vue 3 菜单栏应用。它**独占地**拉起 daemon 进程并在退出时杀掉,同时订阅 socket 消费事件。打包时 daemon 被冻成一个自包含二进制,作为资源塞进 `.app`。

事件协议、各种约束的更多细节见 [`CLAUDE.md`](./CLAUDE.md)。

## 前置条件

**运行**(仅打包安装、使用的话):

- macOS
- **iTerm2**,并且:
  - 打开 Python API:iTerm2 → Preferences → General → Magic → *Enable Python API*
  - 装好 **iTerm2 Shell Integration**(否则收不到命令事件):iTerm2 → Install Shell Integration

**打包 / 开发**还需要:

- [uv](https://docs.astral.sh/uv/)(冻结 Python daemon;会自带 Python 3.14 运行时)
- [bun](https://bun.sh/)(前端 & Tauri 构建钩子)
- [Rust](https://rustup.rs/) 工具链(Tauri 的 Rust 侧)

## 自己打包安装

在仓库根目录跑打包脚本,它会:先用 PyInstaller 把 daemon 冻成自包含二进制,再让 Tauri 把它作为资源连同 UI 一起打进 `.app`,并产出一个可安装的 `.dmg`。

```bash
./build.sh            # 冻 daemon + 打包 UI(默认,出 dmg)
./build.sh daemon     # 只冻 daemon 二进制
./build.sh ui         # 只打包 UI(要求 daemon 已冻好)
```

产物:

- `busy-term-ui/src-tauri/target/release/bundle/dmg/*.dmg` —— 可分发的 dmg
- `busy-term-ui/src-tauri/target/release/bundle/macos/*.app` —— .app

> 打 dmg 时会自动弹开一个访达窗口又关掉,那是 Tauri 在布置 dmg 里的图标位置,属正常现象。

### 安装

1. 打开产出的 `.dmg`,把 **BusyTerm** 拖进「应用程序」。
2. 应用**未签名/未公证**,首次打开会被 Gatekeeper 拦。到 **系统设置 → 隐私与安全性**,在最下方点「仍要打开」放行即可。
3. 启动后图标出现在菜单栏。**从 iTerm2 里启动 iTerm2 会话**并确认 Shell Integration 已装,命令事件才会出现。

> 说明:daemon 由本应用独占拉起。你手动在 iTerm2 里 `uv run main.py` 起的 daemon **不会**被应用接管,应用里仍显示「未启动」。

## 开发

**Daemon**(在 `busy-term-daemon/`,用 uv):

```bash
uv sync               # 装/刷新 .venv
uv run main.py        # 必须在 iTerm2 会话里跑(要向 iTerm2 认证)
```

用 `nc -U /tmp/busy-term.sock` 可以直接看事件流。

**UI**(在 `busy-term-ui/`,用 bun):

```bash
bun install
bun run tauri dev     # 完整桌面应用(先起 Vite:1420,再起 Rust)
bun run dev           # 只在浏览器里跑前端
cargo check           # 在 src-tauri/ 里,只迭代 Rust 时用
```

> `bun run tauri dev` 会拉起打包好的 daemon 二进制,所以开发前先 `./build.sh daemon` 冻一次。

菜单栏图标是纯黑 + alpha 的单色 template 图(`busy-term-ui/src-tauri/icons/tray-*.png`),覆盖对应 PNG 再重新编译即可换图,不用改 Rust 代码。

两个项目都没有配置测试、linter、formatter。

## License

[Apache License 2.0](./LICENSE)
