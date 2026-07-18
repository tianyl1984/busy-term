<script setup>
import { ref, onMounted, onUnmounted, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  enable as enableAutostart,
  disable as disableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";

const status = ref({ state: "stopped", pid: null, last_error: null });
const autostart = ref(false);
const whitelist = ref([]);
const draft = ref("");
const busy = ref(false);
const error = ref("");

let timer = null;

// 和 popover 读的是同一个 daemon_status，两边不会再打架。
const STATE_TEXT = {
  running: "运行中",
  starting: "启动中…",
  external: "运行中（非本应用启动）",
  stopped: "未启动",
};

const label = computed(() => STATE_TEXT[status.value.state]);
const serving = computed(() =>
  ["running", "external"].includes(status.value.state)
);
// 孤儿 daemon 不是我们的子进程，停止/重启这些按钮对它无效，只能提示。
const external = computed(() => status.value.state === "external");

async function refresh() {
  status.value = await invoke("daemon_status");
}

async function run(command) {
  busy.value = true;
  error.value = "";
  try {
    await invoke(command);
  } catch (err) {
    error.value = String(err);
  } finally {
    busy.value = false;
    await refresh();
  }
}

async function toggleAutostart() {
  try {
    if (autostart.value) {
      await disableAutostart();
    } else {
      await enableAutostart();
    }
  } catch (err) {
    error.value = String(err);
  }
  autostart.value = await isAutostartEnabled();
}

async function addEntry() {
  const program = draft.value.trim();
  if (!program) return;
  try {
    whitelist.value = await invoke("whitelist_add", { program });
    draft.value = "";
  } catch (err) {
    error.value = String(err);
  }
}

async function removeEntry(program) {
  try {
    whitelist.value = await invoke("whitelist_remove", { program });
  } catch (err) {
    error.value = String(err);
  }
}

onMounted(async () => {
  autostart.value = await isAutostartEnabled();
  whitelist.value = await invoke("whitelist_get");
  await refresh();
  // daemon 可能自己崩掉，轮询让面板不至于停在陈旧状态上。
  timer = setInterval(refresh, 2000);
});

onUnmounted(() => clearInterval(timer));
</script>

<template>
  <main class="panel">
    <section>
      <h2>通用</h2>
      <div class="card">
        <label class="row clickable">
          <span>开机自动启动</span>
          <span class="switch">
            <input type="checkbox" :checked="autostart" @change="toggleAutostart" />
            <span class="slider"></span>
          </span>
        </label>
      </div>
    </section>

    <section>
      <h2>Daemon</h2>
      <div class="card">
        <div class="row">
          <div class="label">
            <span class="dot" :class="{ on: serving, warn: external }"></span>
            <span>{{ label }}</span>
          </div>
          <span v-if="status.pid" class="meta">PID {{ status.pid }}</span>
        </div>

        <div class="row divided">
          <p v-if="external" class="hint">
            不是本应用启动的（多半是残留的孤儿进程），这里控制不了它。
            需手动结束：<code>pkill -f busy-term-daemon</code>
          </p>
          <p v-else-if="status.state === 'starting'" class="hint">
            进程已起，socket 还没连上。一直停在这里就检查 iTerm2 是否运行、以及自动化授权。
          </p>
          <p v-else-if="error" class="hint err">{{ error }}</p>
          <p v-else-if="status.last_error" class="hint err">{{ status.last_error }}</p>
          <span v-else class="hint">监听 iTerm2 的命令事件</span>

          <div class="actions">
            <!-- 「启动」按需显示：只在没跑的时候出现 -->
            <button
              v-if="status.state === 'stopped'"
              :disabled="busy"
              @click="run('start_daemon')"
            >
              启动
            </button>
            <template v-else-if="!external">
              <button :disabled="busy" @click="run('restart_daemon')">重启</button>
              <button :disabled="busy" @click="run('stop_daemon')">停止</button>
            </template>
          </div>
        </div>
      </div>
    </section>

    <section>
      <h2>白名单</h2>
      <div class="card">
        <div class="row column">
          <p class="hint">按程序名匹配，命中的命令不在菜单栏面板里显示。</p>
          <form class="add" @submit.prevent="addEntry">
            <input v-model="draft" placeholder="程序名，如 ls、vim" />
            <button type="submit" :disabled="!draft.trim()">添加</button>
          </form>
        </div>

        <ul v-if="whitelist.length" class="list">
          <li v-for="program in whitelist" :key="program" class="divided">
            <code>{{ program }}</code>
            <button class="remove" title="移除" @click="removeEntry(program)">×</button>
          </li>
        </ul>
        <p v-else class="hint divided empty">还没有白名单项</p>
      </div>
    </section>
  </main>
</template>

<style scoped>
.panel {
  padding: 18px 16px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

h2 {
  font-size: 13px;
  font-weight: 600;
  margin: 0 0 8px 2px;
}

.card {
  background: #f2f2f7;
  border-radius: 10px;
  overflow: hidden;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 12px 14px;
}

.row.column {
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
}

/* 卡片内分隔线，对齐参考图里 Updates 那组的样式 */
.divided {
  border-top: 1px solid rgba(0, 0, 0, 0.08);
}

.clickable {
  cursor: pointer;
}

.label {
  display: flex;
  align-items: center;
  gap: 8px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #c7c7cc;
  flex: none;
}

.dot.on {
  background: #30d158;
}

.dot.warn {
  background: #ff9f0a;
}

.meta {
  color: #86868b;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.hint {
  margin: 0;
  color: #86868b;
  font-size: 12px;
  line-height: 1.5;
  flex: 1;
}

.hint.err {
  color: #d70015;
}

.hint.empty {
  padding: 12px 14px;
}

code {
  font-family: "SF Mono", ui-monospace, Menlo, monospace;
  font-size: 11px;
  background: rgba(0, 0, 0, 0.06);
  padding: 1px 5px;
  border-radius: 4px;
}

.actions {
  display: flex;
  gap: 6px;
  flex: none;
}

button {
  padding: 5px 12px;
  border-radius: 6px;
  border: none;
  background: #e3e3e8;
  color: #1d1d1f;
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
  white-space: nowrap;
}

button:hover:not(:disabled) {
  background: #d8d8de;
}

button:disabled {
  opacity: 0.4;
  cursor: default;
}

/* iOS 风格开关 */
.switch {
  position: relative;
  width: 40px;
  height: 24px;
  flex: none;
}

.switch input {
  opacity: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  cursor: pointer;
  position: relative;
  z-index: 1;
}

.slider {
  position: absolute;
  inset: 0;
  background: #d1d1d6;
  border-radius: 12px;
  transition: background 0.2s;
  pointer-events: none;
}

.slider::before {
  content: "";
  position: absolute;
  width: 20px;
  height: 20px;
  left: 2px;
  top: 2px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  transition: transform 0.2s;
}

.switch input:checked + .slider {
  background: #0071e3;
}

.switch input:checked + .slider::before {
  transform: translateX(16px);
}

.add {
  display: flex;
  gap: 6px;
}

.add input {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
  border-radius: 6px;
  border: 1px solid rgba(0, 0, 0, 0.12);
  background: #fff;
  font-size: 12px;
  font-family: inherit;
  color: inherit;
}

.add input:focus {
  outline: none;
  border-color: #0071e3;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
  /* 封顶约 5 行，再多就滚动，别让窗口被白名单撑长。 */
  max-height: 160px;
  overflow-y: auto;
}

.list li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 14px;
}

.remove {
  background: none;
  padding: 0 4px;
  font-size: 16px;
  line-height: 1;
  color: #86868b;
}

.remove:hover:not(:disabled) {
  background: none;
  color: #d70015;
}

@media (prefers-color-scheme: dark) {
  .card {
    background: #2c2c2e;
  }

  .divided {
    border-color: rgba(255, 255, 255, 0.1);
  }

  code {
    background: rgba(255, 255, 255, 0.1);
  }

  button {
    background: #3a3a3c;
    color: #f5f5f7;
  }

  button:hover:not(:disabled) {
    background: #48484a;
  }

  .slider {
    background: #48484a;
  }

  .add input {
    background: #1c1c1e;
    border-color: rgba(255, 255, 255, 0.14);
  }

  .hint.err {
    color: #ff453a;
  }
}
</style>
