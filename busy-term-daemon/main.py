"""busy-term daemon: 监听 iTerm2 的命令开始/结束事件，打印到控制台并广播到 unix socket。

需要在 iTerm2 中安装 Shell Integration，命令事件才能被检测到。
运行方式（在 iTerm2 内）：uv run main.py

其他应用可以连接 unix socket 读取事件，每行一条 JSON（NDJSON）：
    {"type": "command_start", "session_id": "...", "command": "ls -l", "ts": 1752...}
    {"type": "command_end",   "session_id": "...", "status": 0,       "ts": 1752...}
socket 路径默认 /tmp/busy-term.sock，可用环境变量 BUSY_TERM_SOCKET 覆盖。
调试：nc -U /tmp/busy-term.sock
"""

import asyncio
import json
import os
import signal
import socket
import time

import iterm2

SOCKET_PATH = os.environ.get("BUSY_TERM_SOCKET", "/tmp/busy-term.sock")

# 只有自己成功 bind 过，退出时才有资格删 socket 文件——否则会删掉别人的。
_own_socket = False


def _socket_is_alive():
    """socket 文件背后是否真有 daemon 在服务。

    连得上 = 有人在服务；ECONNREFUSED = 上次崩溃留下的死文件。
    """
    probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        probe.connect(SOCKET_PATH)
        return True
    except OSError:
        return False
    finally:
        probe.close()


class EventBroker:
    """维护已连接的客户端，并把事件以 NDJSON 广播出去。"""

    def __init__(self):
        self._writers = set()

    async def handle_client(self, reader, writer):
        """新客户端连上来后只做接收方，一直挂到它断开为止。"""
        self._writers.add(writer)
        try:
            # 我们不解析客户端发来的数据，这里只是用读到 EOF 来感知断开。
            await reader.read()
        except ConnectionError:
            pass
        finally:
            self._writers.discard(writer)
            writer.close()

    def broadcast(self, event):
        """把事件写给所有客户端；写不动的客户端直接丢弃，不能拖住监听。"""
        line = (json.dumps(event, ensure_ascii=False) + "\n").encode()
        for writer in list(self._writers):
            try:
                writer.write(line)
            except Exception:
                self._writers.discard(writer)
                writer.close()

    def close(self):
        for writer in list(self._writers):
            writer.close()
        self._writers.clear()


async def monitor_session(connection, broker, session_id):
    """为单个会话监听 COMMAND_START / COMMAND_END 事件。"""
    modes = [
        iterm2.PromptMonitor.Mode.COMMAND_START,
        iterm2.PromptMonitor.Mode.COMMAND_END,
    ]
    try:
        async with iterm2.PromptMonitor(connection, session_id, modes=modes) as mon:
            while True:
                mode, value = await mon.async_get()
                if mode == iterm2.PromptMonitor.Mode.COMMAND_START:
                    print(f"[COMMAND START] session={session_id} command={value!r}")
                    broker.broadcast(
                        {
                            "type": "command_start",
                            "session_id": session_id,
                            "command": value,
                            "ts": time.time(),
                        }
                    )
                elif mode == iterm2.PromptMonitor.Mode.COMMAND_END:
                    print(f"[COMMAND END]   session={session_id} status={value}")
                    broker.broadcast(
                        {
                            "type": "command_end",
                            "session_id": session_id,
                            "status": value,
                            "ts": time.time(),
                        }
                    )
    except asyncio.CancelledError:
        # 退出时会话监听任务被取消，属于正常结束，不必冒泡成噪音。
        pass


async def monitor_session_terminations(connection, broker):
    """监听会话结束，为结束的会话补发一条 command_end。

    像 `exit` 这样直接结束 shell 的命令，会话在下一个 prompt 出现之前就没了。
    而 iTerm2 Shell Integration 的 COMMAND_END 是挂在「下一个 prompt」上报的——
    prompt 不会再来，COMMAND_END 也就永远不会触发。结果这条命令在客户端里会
    一直停在「正在执行」。这里靠会话结束事件兜底，替它补一条 command_end。

    对没有在跑命令的会话补发也无所谓：客户端按 session_id 删除，本来就没有对应
    项时是空操作。
    """
    async with iterm2.SessionTerminationMonitor(connection) as mon:
        while True:
            session_id = await mon.async_get()
            print(f"[SESSION END]   session={session_id}")
            broker.broadcast(
                {
                    "type": "command_end",
                    "session_id": session_id,
                    "status": None,
                    "ts": time.time(),
                }
            )


async def start_socket_server(broker):
    """在 SOCKET_PATH 上开 unix socket 服务端。"""
    global _own_socket
    if os.path.exists(SOCKET_PATH):
        # 文件存在有两种可能：另一个 daemon 正在服务，或者上次崩溃留下的死文件。
        # 必须先连一下区分开——无条件 unlink 会把活着的 daemon 悄悄废掉：
        # 它的进程还在、还监听着，但路径已经指不到它，谁也连不上了。
        if _socket_is_alive():
            raise RuntimeError(
                f"{SOCKET_PATH} 上已经有一个 daemon 在服务了，本进程退出。"
            )
        os.unlink(SOCKET_PATH)
    server = await asyncio.start_unix_server(broker.handle_client, path=SOCKET_PATH)
    os.chmod(SOCKET_PATH, 0o600)
    _own_socket = True
    return server


async def main(connection):
    app = await iterm2.async_get_app(connection)
    broker = EventBroker()
    await start_socket_server(broker)

    async def task(session_id):
        await monitor_session(connection, broker, session_id)

    print(f"busy-term daemon 已启动，正在监听命令事件……socket: {SOCKET_PATH}")
    # 会话结束监听要独立跑：async_foreach_session_create_task 会一直阻塞在这里，
    # 所以先把它挂成后台任务再进去。
    asyncio.create_task(monitor_session_terminations(connection, broker))
    await iterm2.EachSessionOnceMonitor.async_foreach_session_create_task(app, task)


def _handle_sigterm(signum, frame):
    """被进程管理器（比如 busy-term-ui）SIGTERM 掉时，走和 Ctrl+C 相同的退出路径。

    默认的 SIGTERM 处置是直接终止进程，下面的 finally 不会执行，socket 文件就会残留。
    """
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, _handle_sigterm)
    # iterm2.run_forever 自己持有事件循环，Ctrl+C 会让 KeyboardInterrupt 直接从
    # loop.run_until_complete 里冒出来，所以只能在最外层兜住。
    try:
        iterm2.run_forever(main)
    except KeyboardInterrupt:
        print("\nbusy-term daemon 已停止")
    finally:
        # 只删自己 bind 的那个。如果是因为「已有 daemon 在服务」而退出，
        # 这里的文件是别人的，删了就等于把对方废掉。
        if _own_socket and os.path.exists(SOCKET_PATH):
            os.unlink(SOCKET_PATH)
