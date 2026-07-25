<script setup lang="ts">
import type { BridgeStatus } from "../types";

const props = defineProps<{
  status: BridgeStatus;
  loading: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
}>();

function statusText(status: BridgeStatus): string {
  if (status.startsWith("Error|")) {
    return status.slice("Error|".length) || "错误";
  }
  if (status.startsWith("Error")) return status;
  const map: Record<string, string> = {
    Disconnected: "未连接",
    Connecting: "连接中...",
    Connected: "已连接",
  };
  return map[status] || status;
}

function statusClass(status: BridgeStatus): string {
  if (status === "Connected") return "connected";
  if (status === "Connecting") return "connecting";
  if (status.startsWith("Error")) return "error";
  return "disconnected";
}

function buttonText(status: BridgeStatus): string {
  if (status === "Connected") return "断开连接";
  if (status === "Connecting") return "连接中...";
  return "连接设备";
}
</script>

<template>
  <div class="device-status">
    <span :class="['status-indicator', statusClass(status)]">
      <span class="dot"></span>
      {{ statusText(status) }}
    </span>
    <button
      :class="['btn', status === 'Connected' ? 'btn-danger' : 'btn-primary']"
      :disabled="loading || status === 'Connecting'"
      @click="emit('toggle')"
    >
      {{ loading ? "处理中..." : buttonText(status) }}
    </button>
  </div>
</template>

<style scoped>
.device-status {
  display: flex;
  align-items: center;
  gap: 12px;
}

.status-indicator {
  font-size: 13px;
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}

.status-indicator.connected { color: var(--success); }
.status-indicator.connected .dot { background: var(--success); }

.status-indicator.connecting { color: var(--warning); }
.status-indicator.connecting .dot {
  background: var(--warning);
  animation: pulse 1s ease-in-out infinite;
}

.status-indicator.disconnected { color: var(--text-secondary); }
.status-indicator.disconnected .dot { background: #94a3b8; }

.status-indicator.error { color: var(--danger); }
.status-indicator.error .dot { background: var(--danger); }

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.btn {
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.15s ease;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--primary);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  background: var(--primary-dark);
}

.btn-danger {
  background: var(--danger);
  color: #fff;
}

.btn-danger:hover:not(:disabled) {
  background: #dc2626;
}
</style>
