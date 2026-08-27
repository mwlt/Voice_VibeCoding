import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceConfig, BridgeType, KeyAction } from "../types";

type ConfigLoadState = "pending" | "loading" | "ready" | "error";

export const useConfigStore = defineStore("config", () => {
  const configs = ref<Record<BridgeType, DeviceConfig | null>>({
    xiaomi: null,
    t1: null,
    hanvon: null,
  });

  const loadStates = ref<Record<BridgeType, ConfigLoadState>>({
    xiaomi: "pending",
    t1: "pending",
    hanvon: "pending",
  });

  const loadErrors = ref<Record<BridgeType, string | null>>({
    xiaomi: null,
    t1: null,
    hanvon: null,
  });

  const saving = ref(false);

  async function loadConfig(type: BridgeType) {
    loadStates.value[type] = "loading";
    loadErrors.value[type] = null;
    try {
      const config = await invoke<DeviceConfig>("get_config", {
        bridgeType: type,
      });
      configs.value[type] = config;
      loadStates.value[type] = "ready";
    } catch (e) {
      const msg =
        e instanceof Error ? e.message : typeof e === "string" ? e : String(e);
      loadErrors.value[type] = msg;
      loadStates.value[type] = "error";
      console.error(`Failed to load ${type} config:`, e);
    }
  }

  async function saveConfig(type: BridgeType, config: DeviceConfig) {
    saving.value = true;
    try {
      await invoke("save_config", { bridgeType: type, config });
      configs.value[type] = config;
    } catch (e) {
      console.error(`Failed to save ${type} config:`, e);
    } finally {
      saving.value = false;
    }
  }

  async function updateKeyMapping(
    type: BridgeType,
    buttonId: string,
    action: KeyAction
  ) {
    try {
      await invoke("update_key_mapping", {
        bridgeType: type,
        buttonId,
        action,
      });
      if (configs.value[type]) {
        configs.value[type]!.button_bindings[buttonId] = action;
      }
    } catch (e) {
      console.error(`Failed to update key mapping:`, e);
    }
  }

  return {
    configs,
    loadStates,
    loadErrors,
    saving,
    loadConfig,
    saveConfig,
    updateKeyMapping,
  };
});
