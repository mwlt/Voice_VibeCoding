import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceInfo, BridgeType, BridgeStatus } from "../types";

export const useBridgeStore = defineStore("bridge", () => {
  const devices = ref<Record<BridgeType, DeviceInfo>>({
    xiaomi: {
      bridge_type: "xiaomi",
      status: "Disconnected",
      device_name: null,
      device_address: null,
      battery_level: null,
    },
    t1: {
      bridge_type: "t1",
      status: "Disconnected",
      device_name: null,
      device_address: null,
      battery_level: null,
    },
    hanvon: {
      bridge_type: "hanvon",
      status: "Disconnected",
      device_name: null,
      device_address: null,
      battery_level: null,
    },
  });

  const loading = ref<Record<BridgeType, boolean>>({
    xiaomi: false,
    t1: false,
    hanvon: false,
  });

  async function refreshStatus(type: BridgeType) {
    try {
      const info = await invoke<DeviceInfo>("get_device_status", {
        bridgeType: type,
      });
      devices.value[type] = info;
    } catch (e) {
      // Silently fail in browser dev mode (Tauri API not available)
      console.warn(`Failed to get ${type} status (expected in browser dev):`, e);
    }
  }

  async function refreshAll() {
    await Promise.all([
      refreshStatus("xiaomi"),
      refreshStatus("t1"),
      refreshStatus("hanvon"),
    ]);
  }

  async function startBridge(type: BridgeType) {
    loading.value[type] = true;
    devices.value[type].status = "Connecting";
    try {
      await invoke("start_bridge", { bridgeType: type });
      await refreshStatus(type);
    } catch (e) {
      devices.value[type].status = `Error: ${e}` as BridgeStatus;
      console.error(`Failed to start ${type}:`, e);
    } finally {
      loading.value[type] = false;
    }
  }

  async function stopBridge(type: BridgeType) {
    loading.value[type] = true;
    try {
      await invoke("stop_bridge", { bridgeType: type });
      devices.value[type].status = "Disconnected";
    } catch (e) {
      console.error(`Failed to stop ${type}:`, e);
    } finally {
      loading.value[type] = false;
    }
  }

  function statusLabel(status: BridgeStatus): string {
    if (status.startsWith("Error|")) {
      return status.slice("Error|".length) || "错误";
    }
    if (status.startsWith("Error")) return status;
    const map: Record<string, string> = {
      Disconnected: "未连接",
      Connecting: "连接中...",
      Connected: "已连接",
      Error: "错误",
    };
    return map[status] || status;
  }

  function statusColor(status: BridgeStatus): string {
    if (status === "Connected") return "var(--success)";
    if (status === "Connecting") return "var(--warning)";
    if (status.startsWith("Error")) return "var(--danger)";
    return "var(--text-secondary)";
  }

  return {
    devices,
    loading,
    refreshStatus,
    refreshAll,
    startBridge,
    stopBridge,
    statusLabel,
    statusColor,
  };
});
