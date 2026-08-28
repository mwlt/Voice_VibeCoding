<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { storeToRefs } from "pinia";
import { useBridgeStore } from "../stores/bridge";
import { useAppUpdateStore } from "../stores/appUpdate";
import type { BridgeStatus } from "../types";

const route = useRoute();
const router = useRouter();
const bridge = useBridgeStore();
const appUpdate = useAppUpdateStore();
const { updateInfo, shouldShowPassivePrompt } = storeToRefs(appUpdate);

const showQuitConfirm = ref(false);
const quitting = ref(false);

function statusClass(status: BridgeStatus): string {
  if (status === "Connected") return "connected";
  if (status === "Connecting") return "connecting";
  if (status.startsWith("Error")) return "error";
  return "disconnected";
}

const deviceItems = [
  { path: "/xiaomi", label: "小米2 pro", type: "xiaomi" as const },
  { path: "/t1", label: "T1 [开发中]", type: "t1" as const },
  { path: "/v60", label: "V60 [开发中]", type: "hanvon" as const },
];

const appVersion = ref("…");

onMounted(async () => {
  try {
    appVersion.value = `v${await getVersion()}`;
  } catch {
    appVersion.value = "v1.5.7";
  }
});

function navigate(path: string) {
  router.push(path);
}

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + "/");
}

function openQuitConfirm() {
  showQuitConfirm.value = true;
}

function cancelQuit() {
  if (quitting.value) return;
  showQuitConfirm.value = false;
}

async function confirmQuit() {
  if (quitting.value) return;
  quitting.value = true;
  try {
    await invoke("quit_application");
  } catch (e) {
    console.error("quit_application failed:", e);
    quitting.value = false;
  }
}
</script>

<template>
  <header class="topnav">
    <div class="brand">
      <span class="brand-name">Voice VibeCoding</span>
      <span class="brand-ver">{{ appVersion }}</span>
      <template v-if="shouldShowPassivePrompt">
        <span class="brand-update-badge">新版本 V{{ updateInfo!.latestVersion }}</span>
        <button type="button" class="brand-update-btn" @click="appUpdate.openModal()">
          查看更新内容
        </button>
      </template>
    </div>

    <nav class="nav-row">
      <button
        v-for="item in deviceItems"
        :key="item.path"
        type="button"
        :class="['nav-item', { active: isActive(item.path) }]"
        @click="navigate(item.path)"
      >
        <span
          :class="['dot', statusClass(bridge.devices[item.type].status)]"
          :title="bridge.statusLabel(bridge.devices[item.type].status)"
        />
        <span class="nav-label">{{ item.label }}</span>
      </button>
    </nav>

    <div class="nav-actions">
      <button
        type="button"
        :class="['nav-item', { active: isActive('/settings') }]"
        @click="navigate('/settings')"
      >
        <span class="nav-label">设置</span>
      </button>
      <button type="button" class="nav-item nav-exit" @click="openQuitConfirm">
        <span class="nav-label">退出</span>
      </button>
    </div>
  </header>

  <Teleport to="body">
    <div
      v-if="showQuitConfirm"
      class="quit-backdrop"
      role="presentation"
      @click.self="cancelQuit"
    >
      <div class="quit-dialog" role="dialog" aria-modal="true" aria-labelledby="quit-title">
        <h3 id="quit-title">退出应用？</h3>
        <p>将彻底关闭软件（不会最小化到托盘）。确定要退出吗？</p>
        <div class="quit-actions">
          <button type="button" class="quit-btn quit-btn-ghost" :disabled="quitting" @click="cancelQuit">
            取消
          </button>
          <button type="button" class="quit-btn quit-btn-danger" :disabled="quitting" @click="confirmQuit">
            {{ quitting ? "退出中..." : "退出" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.topnav {
  display: flex;
  align-items: center;
  gap: 20px;
  height: 48px;
  padding: 0 20px;
  background: var(--sidebar-bg);
  color: var(--sidebar-text);
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  user-select: none;
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.brand-name {
  font-size: 13px;
  font-weight: 600;
  color: #fff;
  letter-spacing: 0.01em;
  white-space: nowrap;
}

.brand-ver {
  font-size: 11px;
  color: #94a3b8;
}

.brand-update-badge {
  font-size: 11px;
  font-weight: 600;
  color: #93c5fd;
  white-space: nowrap;
}

.brand-update-btn {
  height: 24px;
  padding: 0 8px;
  border: 1px solid rgba(147, 197, 253, 0.45);
  border-radius: 4px;
  background: rgba(59, 130, 246, 0.15);
  color: #bfdbfe;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  white-space: nowrap;
  transition: background 0.15s ease, color 0.15s ease;
}

.brand-update-btn:hover {
  background: rgba(59, 130, 246, 0.28);
  color: #fff;
}

.nav-row {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  overflow-x: auto;
  flex: 1;
}

.nav-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
  flex-shrink: 0;
}

.nav-item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 12px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--sidebar-text);
  cursor: pointer;
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  transition: background 0.15s ease, color 0.15s ease;
}

.nav-item:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #fff;
}

.nav-item.active {
  background: var(--sidebar-active);
  color: #fff;
}

.nav-exit:hover {
  background: rgba(239, 68, 68, 0.18);
  color: #fecaca;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
  background: #64748b;
}

.dot.connected {
  background: var(--success);
}

.dot.connecting {
  background: var(--warning);
}

.dot.error {
  background: var(--danger);
}

.dot.disconnected {
  background: #64748b;
}

.quit-backdrop {
  position: fixed;
  inset: 0;
  z-index: 3000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
  background: rgba(15, 23, 42, 0.45);
}

.quit-dialog {
  width: min(360px, 100%);
  padding: 18px 18px 14px;
  border-radius: 10px;
  background: #fff;
  box-shadow: 0 12px 40px rgba(15, 23, 42, 0.25);
  color: var(--text, #1e293b);
}

.quit-dialog h3 {
  margin: 0 0 8px;
  font-size: 16px;
  font-weight: 600;
}

.quit-dialog p {
  margin: 0 0 16px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-secondary, #64748b);
}

.quit-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.quit-btn {
  height: 32px;
  padding: 0 14px;
  border-radius: 6px;
  border: 1px solid transparent;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}

.quit-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.quit-btn-ghost {
  background: #fff;
  border-color: var(--border, #e2e8f0);
  color: var(--text, #1e293b);
}

.quit-btn-ghost:hover:not(:disabled) {
  background: #f8fafc;
}

.quit-btn-danger {
  background: var(--danger, #ef4444);
  color: #fff;
}

.quit-btn-danger:hover:not(:disabled) {
  filter: brightness(0.95);
}
</style>
