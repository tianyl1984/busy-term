<script setup>
import { ref, onMounted, onUnmounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const commands = ref([]);
const status = ref({ state: "stopped", pid: null, last_error: null });
// 单独存一份「现在几点」，让已运行时长每秒自己往前走，不用等下一次轮询。
const now = ref(Date.now() / 1000);
// 每次窗口显示就自增，用作 :key 强制重建 DOM，让入场动画重新播放。
const showCount = ref(0);

let pollTimer = null;
let tickTimer = null;
let unlisten = null;

async function refresh() {
  // 状态和 daemon.rs 的 daemon_status 是同一个来源，和设置面板读的完全一致。
  [commands.value, status.value] = await Promise.all([
    invoke("running_commands"),
    invoke("daemon_status"),
  ]);
}

function elapsed(startedAt) {
  const secs = Math.max(0, Math.floor(now.value - startedAt));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${secs % 60}s`;
  return `${Math.floor(mins / 60)}h ${mins % 60}m`;
}

// 和 config.rs 的 program_name 保持一致：取第一个词、去掉路径。只用于按钮提示文案，
// 真正入库的归一化在 Rust 那边做。
function programName(command) {
  const first = command.trim().split(/\s+/)[0] ?? "";
  return first.split("/").pop() || first;
}

// 后端 whitelist_add 会自己收敛成程序名，所以整条命令直接传过去即可。
async function hide(command) {
  try {
    await invoke("whitelist_add", { program: command });
    // 立刻刷新，别等下一次轮询才让它消失。
    await refresh();
  } catch (err) {
    console.error("加入白名单失败", err);
  }
}

const STATE_TEXT = {
  running: "没有正在执行的命令",
  external: "没有正在执行的命令",
  starting: "daemon 启动中…",
  stopped: "daemon 未运行",
};

const emptyHint = computed(() => STATE_TEXT[status.value.state]);
// external = 有 daemon 在服务，但不是本 app 启动的（多半是上次残留的孤儿进程）。
const serving = computed(() =>
  ["running", "external"].includes(status.value.state)
);

onMounted(async () => {
  refresh();
  pollTimer = setInterval(refresh, 1000);
  tickTimer = setInterval(() => (now.value = Date.now() / 1000), 1000);
  unlisten = await listen("popover-shown", () => {
    // 显示的瞬间顺便刷一次，别让第一眼看到的是上次关掉时的旧数据。
    refresh();
    showCount.value += 1;
  });
});

onUnmounted(() => {
  clearInterval(pollTimer);
  clearInterval(tickTimer);
  unlisten?.();
});
</script>

<template>
  <div class="popover" :key="showCount">
    <header>
      <span class="title">BusyTerm</span>
      <span
        class="dot"
        :class="{ on: serving, warn: status.state === 'external' }"
        :title="serving ? 'daemon 运行中' : 'daemon 未运行'"
      ></span>
      <button class="icon" title="设置" @click="invoke('open_settings')">⚙</button>
    </header>

    <main>
      <ul v-if="commands.length" class="list">
        <li v-for="cmd in commands" :key="cmd.session_id">
          <span class="cmd" :title="cmd.command">{{ cmd.command }}</span>
          <span class="time">{{ elapsed(cmd.started_at) }}</span>
          <button
            class="hide"
            :title="`不再显示 ${programName(cmd.command)}`"
            @click="hide(cmd.command)"
          >
            ⊘
          </button>
        </li>
      </ul>
      <p v-else class="empty">{{ emptyHint }}</p>
    </main>

    <footer>
      <button class="quit" @click="invoke('quit_app')">退出</button>
    </footer>
  </div>
</template>

<style scoped>
/* 窗口是透明的，圆角、阴影都由这一层画。留出 margin 给阴影，
   否则 overflow:hidden 会把阴影裁掉。 */
.popover {
  height: calc(100vh - 16px);
  margin: 8px;
  border-radius: 12px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: #f5f5f7;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
  transform-origin: top center;
  animation: popover-in 140ms ease-out;
}

@keyframes popover-in {
  from {
    opacity: 0;
    transform: translateY(-6px) scale(0.97);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

/* 系统开了「减弱动态效果」就别硬播动画。 */
@media (prefers-reduced-motion: reduce) {
  .popover {
    animation: none;
  }
}

header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 14px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.08);
}

.title {
  font-weight: 600;
  font-size: 14px;
  flex: 1;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #c7c7cc;
  flex: none;
}

.dot.on {
  background: #30d158;
}

/* 孤儿 daemon：在服务，但不受本 app 控制，值得用颜色提示一下。 */
.dot.warn {
  background: #ff9f0a;
}

.icon {
  border: none;
  background: none;
  cursor: pointer;
  font-size: 15px;
  color: #6e6e73;
  padding: 2px 4px;
  line-height: 1;
}

.icon:hover {
  color: #1d1d1f;
}

main {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.list li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  background: #fff;
}

.cmd {
  flex: 1;
  font-family: "SF Mono", ui-monospace, Menlo, monospace;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.time {
  color: #86868b;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  flex: none;
}

/* 平时藏起来，鼠标移到那一行才露出来——列表本身保持干净。
   用 opacity 而不是 display，位置就不会在 hover 时跳。 */
.hide {
  border: none;
  background: none;
  padding: 0 2px;
  line-height: 1;
  font-size: 13px;
  color: #86868b;
  cursor: pointer;
  flex: none;
  opacity: 0;
  transition: opacity 120ms ease-out;
}

.list li:hover .hide,
.hide:focus-visible {
  opacity: 1;
}

.hide:hover {
  color: #1d1d1f;
}

.empty {
  color: #86868b;
  font-size: 13px;
  text-align: center;
  margin: 24px 0;
}

footer {
  border-top: 1px solid rgba(0, 0, 0, 0.08);
  padding: 6px;
}

.quit {
  width: 100%;
  border: none;
  background: none;
  padding: 8px;
  border-radius: 7px;
  font-size: 13px;
  font-family: inherit;
  color: #1d1d1f;
  cursor: pointer;
  text-align: left;
}

.quit:hover {
  background: rgba(0, 0, 0, 0.06);
}

@media (prefers-color-scheme: dark) {
  .popover {
    background: #1e1e1e;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }

  header,
  footer {
    border-color: rgba(255, 255, 255, 0.1);
  }

  .list li {
    background: #2c2c2e;
  }

  .icon {
    color: #98989d;
  }

  .icon:hover,
  .hide:hover {
    color: #f5f5f7;
  }

  .quit {
    color: #f5f5f7;
  }

  .quit:hover {
    background: rgba(255, 255, 255, 0.08);
  }
}
</style>
