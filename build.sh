#!/usr/bin/env bash
#
# 一键打包 busy-term:先把 daemon 冻成自包含二进制,再让 Tauri 把它作为资源
# 连同 UI 一起打进 .app,并出一个可安装的 .dmg。
#
# 产物:
#   busy-term-daemon/dist/busy-term-daemon                          —— 冻好的 daemon
#   busy-term-ui/src-tauri/target/release/bundle/dmg/*.dmg          —— 可分发的 dmg
#   busy-term-ui/src-tauri/target/release/bundle/macos/*.app        —— .app
#
# 暂不处理签名/公证:用户首次打开需在「系统设置 → 隐私与安全性」里放行。
#
# 用法:./build.sh            冻 daemon + 打包 UI(默认)
#       ./build.sh daemon     只冻 daemon
#       ./build.sh ui         只打包 UI(要求 daemon 已冻好)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DAEMON_DIR="$ROOT/busy-term-daemon"
UI_DIR="$ROOT/busy-term-ui"
DAEMON_BIN="$DAEMON_DIR/dist/busy-term-daemon"
# Tauri 的 resource 路径解析不认 ../,所以把冻好的二进制拷进 src-tauri 内部再引用。
RESOURCE_BIN="$UI_DIR/src-tauri/resources/busy-term-daemon"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "找不到 $1,请先安装。"; }

build_daemon() {
  need uv
  log "冻结 daemon(PyInstaller,自带 Python 运行时)…"
  cd "$DAEMON_DIR"
  # 干净重建,免得旧的 spec/build 缓存捣乱。
  rm -rf build dist busy-term-daemon.spec
  uv run --with pyinstaller pyinstaller \
    --onefile \
    --name busy-term-daemon \
    --collect-all iterm2 \
    --collect-all websockets \
    --collect-all google.protobuf \
    --noconfirm \
    main.py
  [ -f "$DAEMON_BIN" ] || die "PyInstaller 没产出 $DAEMON_BIN"
  chmod +x "$DAEMON_BIN"
  mkdir -p "$(dirname "$RESOURCE_BIN")"
  cp "$DAEMON_BIN" "$RESOURCE_BIN"
  log "daemon 冻好了:$DAEMON_BIN → $RESOURCE_BIN"
}

build_ui() {
  need bun
  [ -f "$RESOURCE_BIN" ] || die "daemon 资源不存在($RESOURCE_BIN),先跑 './build.sh daemon'。"
  log "打包 UI(Tauri,把 daemon 作为资源一起塞进 .app)…"
  cd "$UI_DIR"
  bun install
  # tauri.conf.json 的 beforeBuildCommand 会先跑 `bun run build` 构建前端。
  bun run tauri build
}

case "${1:-all}" in
  daemon) build_daemon ;;
  ui)     build_ui ;;
  all)    build_daemon; build_ui ;;
  *)      die "未知参数:$1(可用:daemon | ui | all)" ;;
esac

log "完成。dmg 在:"
ls -1 "$UI_DIR/src-tauri/target/release/bundle/dmg/"*.dmg 2>/dev/null \
  || echo "  (没找到 dmg,检查上面 Tauri 的输出)"
