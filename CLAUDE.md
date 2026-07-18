# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

busy-term is a macOS-only iTerm2 terminal-activity monitor, in two projects:

- `busy-term-daemon/` — Python 3.14 daemon that subscribes to iTerm2's Python API, prints command start/end events, and broadcasts them as JSON over a unix socket (`main.py`).
- `busy-term-ui/` — Tauri 2 + Vue 3 + Vite menu bar app. Owns the daemon process and consumes its socket.

The unix socket is the seam between the two: the daemon publishes, the UI subscribes. See "Event protocol" below.

## UI architecture

macOS menu bar app — no Dock icon (`ActivationPolicy::Accessory` in `lib.rs`), so there is **no app menu and `Cmd+Q` does nothing**; quitting goes through the tray menu or the popover's 退出 button.

Two windows, both driven from the same `index.html`; `App.vue` picks the view from `getCurrentWindow().label`:

- `popover` — left-click the tray icon. Undecorated, positioned under the tray icon from the click event's `rect`, hides on blur. Lists currently-running commands (`Popover.vue`).
- `main` — the settings window, opened from the gear or the tray menu. Hidden at launch; closing only hides it (`Settings.vue`).

Rust side, `src-tauri/src/`:

- `daemon.rs` — owns the daemon child process (start/stop/restart/status). The app spawns it exclusively and kills it on exit; a daemon started by hand in iTerm2 is *not* adopted and shows as 未启动. It spawns a **frozen single-file binary** (`daemon_bin()`), not `uv`/`python` — resolved from, in order, `BUSY_TERM_DAEMON_BIN`, the bundled resource (`.app/Contents/Resources/busy-term-daemon`, found via `current_exe`), then the dev fallback `busy-term-daemon/dist/busy-term-daemon`. Freeze it with `build.sh` before packaging or `bun run tauri dev`.
- `events.rs` — background thread that connects to the daemon socket, tracks `session_id → running command`, and reconnects every second. `running_commands` returns that plus a `connected` flag.

Two distinct notions of "is the daemon up" coexist deliberately: `daemon_status.running` is child-process liveness (drives the settings buttons), while `running_commands.connected` is socket connectivity (drives the popover). The socket answer is the stronger one — the process can be alive but not yet serving.

### Gotchas worth knowing

- **GUI apps get a minimal PATH** (`/usr/bin:/bin:/usr/sbin:/sbin`) — `launchctl getenv PATH` is unset here, and there's no `uv`/`python` on it. That's exactly why the daemon ships as a frozen self-contained binary: nothing to resolve on PATH at runtime. Never rely on PATH for spawning.
- **Tauri resource paths can't use `../`** and are validated at build-script time, so `cargo check` / `tauri build` *fail* unless `src-tauri/resources/busy-term-daemon` exists. `build.sh` freezes the daemon and copies it there; the file is gitignored.
- **Bundled resources can lose the executable bit.** `start()` re-chmods the binary `+x` before spawning as a safety net.
- The daemon child is put in its own session via `setsid()` in `pre_exec` before `stop()` signals the **process group**. Without setsid the child would share the app's group and `kill(-pid)` would take the app down with it.
- Tray icons can be silently pushed off-screen by a crowded menu bar / the notch. An invisible icon is not necessarily a bug.

## Event protocol

The daemon serves a unix socket at `/tmp/busy-term.sock` (override with `BUSY_TERM_SOCKET`), mode `0600`. Clients connect and read; the daemon never reads from them. Payload is NDJSON — one JSON object per line:

```json
{"type": "command_start", "session_id": "26BF...", "command": "echo hi", "ts": 1784277062.23}
{"type": "command_end",   "session_id": "26BF...", "status": 0,          "ts": 1784277062.24}
```

Debug with `nc -U /tmp/busy-term.sock`. Notes for anyone writing a client:

- Broadcast is fire-and-forget: a client that can't be written to is dropped, and events emitted while nobody is connected are lost. There is no replay or buffering.
- A newly opened session emits a spurious `command_end` with `status: 0` when its first prompt appears — that's iTerm2 Shell Integration reporting the "previous" command, not a real one. Clients that pair start/end events must tolerate an unmatched `command_end`.
- The daemon unlinks a stale socket file before binding, and removes it on exit.

## Commands

Packaging — from the repo root, produces a `.dmg`:

```bash
./build.sh            # freeze daemon (PyInstaller) + bundle UI (Tauri) → dmg
./build.sh daemon     # only freeze the daemon binary
./build.sh ui         # only bundle the app (daemon must be frozen already)
```

Output: `busy-term-ui/src-tauri/target/release/bundle/dmg/*.dmg`. Unsigned — first launch needs 系统设置 → 隐私与安全性 to allow it.

Daemon (from `busy-term-daemon/`, uses uv):

```bash
uv run main.py        # must be run from inside iTerm2 — see constraints below
uv sync               # install/refresh .venv
```

UI (from `busy-term-ui/`, uses **bun** — `tauri.conf.json` hardcodes `bun run dev` / `bun run build` as the Tauri hooks, so don't switch package managers casually):

```bash
bun run tauri dev     # full desktop app (spawns Vite on :1420, then Rust)
bun run dev           # frontend only in a browser
bun run tauri build   # bundle the app
cargo check           # from src-tauri/, for Rust-only iteration
```

There are no tests, linters, or formatters configured in either project.

## Daemon constraints

These are easy to trip over and produce silent no-ops rather than errors:

- The daemon talks to iTerm2 over its local Python API. iTerm2 must be running with the Python API enabled (Preferences → General → Magic), and it generally must be **launched from within an iTerm2 session** to authenticate.
- Command events only fire if **iTerm2 Shell Integration** is installed in the user's shell. Without it `PromptMonitor` connects fine but never yields events.
- `iterm2.run_forever(main)` owns the event loop; `EachSessionOnceMonitor.async_foreach_session_create_task` spawns one long-lived `PromptMonitor` task per session, including sessions created later. Per-session monitoring code must stay in an infinite `async_get()` loop — returning ends monitoring for that session.
- Python 3.14 is pinned via `.python-version`; `websockets`/`protobuf` in the venv are transitive deps of `iterm2`, not direct ones.

## Tauri notes

- Frontend and Rust are separate crates' worth of config: `src-tauri/Cargo.toml` for Rust deps, `src-tauri/tauri.conf.json` for windows/bundle/build hooks, `src-tauri/capabilities/default.json` for the permission allowlist. New Tauri plugins usually need edits in all three.
- Rust commands are registered in `src-tauri/src/lib.rs` via `invoke_handler(tauri::generate_handler![...])` and called from Vue with `invoke("name", { args })`. `main.rs` only delegates to `lib.rs::run()`.
- Vite's dev port 1420 is `strictPort: true` and must match `devUrl` in `tauri.conf.json`.

## Conventions

The daemon's docstrings and comments are written in Chinese; match that when editing it.
