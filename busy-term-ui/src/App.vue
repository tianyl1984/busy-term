<script setup>
import { getCurrentWindow } from "@tauri-apps/api/window";
import Popover from "./Popover.vue";
import Settings from "./Settings.vue";

// 两个窗口共用同一个 index.html，靠 label 决定渲染哪个界面。
const isPopover = getCurrentWindow().label === "popover";

// popover 是透明窗口，圆角之外的部分必须真透明；设置窗口则要保持不透明背景。
// 两个窗口共用一份全局样式，所以把 label 挂到根节点上做区分。
if (isPopover) {
  document.documentElement.dataset.window = "popover";
}
</script>

<template>
  <Popover v-if="isPopover" />
  <Settings v-else />
</template>

<style>
:root {
  font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif;
  font-size: 14px;
  color: #1d1d1f;
  /* 设置窗口：白底配浅灰卡片。popover 自己是透明的，不受影响。 */
  background-color: #ffffff;
  -webkit-font-smoothing: antialiased;
}

body {
  margin: 0;
}

:root[data-window="popover"],
:root[data-window="popover"] body {
  background: transparent;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f5f5f7;
    background-color: #1e1e1e;
  }

  :root[data-window="popover"],
  :root[data-window="popover"] body {
    background: transparent;
  }
}
</style>
