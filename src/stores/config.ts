import { defineStore } from "pinia";
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DeviceConfig, BridgeType, KeyAction } from "../types";

function emptyConfig(): DeviceConfig {
  return {
    button_aliases: {},
    button_bindings: {},
    voice_hotkey: null,
    trigger_mode: "Hold",
    bluetooth_address: null,
    gain_db: 10,
    voice_shortcut_enabled: true,
  };
}

export const useConfigStore = defineStore("config", () => {
  const configs = ref<Record<BridgeType, DeviceConfig | null>>({
    xiaomi: null,
    t1: null,
    hanvon: null,
  });

  // 加载失败时置 true，供 UI 提示「已使用默认配置」，而非整块空白
  const loadErrors = ref<Record<BridgeType, boolean>>({
    xiaomi: false,
    t1: false,
    hanvon: false,
  });

  const saving = ref(false);

  async function loadConfig(type: BridgeType) {
    try {
      const config = await invoke<DeviceConfig>("get_config", {
        bridgeType: type,
      });
      configs.value[type] = config;
      loadErrors.value[type] = false;
    } catch (e) {
      console.error(`Failed to load ${type} config:`, e);
      // 兜底：用空默认配置，保证映射区仍能渲染，而非保持 null 空白
      configs.value[type] = emptyConfig();
      loadErrors.value[type] = true;
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
    loadErrors,
    saving,
    loadConfig,
    saveConfig,
    updateKeyMapping,
  };
});
