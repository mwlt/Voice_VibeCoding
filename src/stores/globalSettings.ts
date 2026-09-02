import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { GlobalSettings } from "../types";

const DEFAULT_SETTINGS: GlobalSettings = {
  autostart: false,
  language: "zh-CN",
  minimize_to_tray: true,
  start_minimized_to_tray: false,
  hide_dev_menus: true,
};

export const useGlobalSettingsStore = defineStore("globalSettings", () => {
  const settings = ref<GlobalSettings>({ ...DEFAULT_SETTINGS });
  const loaded = ref(false);
  const saving = ref(false);

  const hideDevMenus = computed(() => settings.value.hide_dev_menus ?? true);

  async function load() {
    try {
      const s = await invoke<GlobalSettings>("get_global_settings");
      settings.value = { ...DEFAULT_SETTINGS, ...s };
      loaded.value = true;
    } catch (e) {
      console.error("Failed to load global settings:", e);
    }
  }

  async function save(): Promise<boolean> {
    if (saving.value) return false;
    saving.value = true;
    try {
      await invoke("save_global_settings", { settings: settings.value });
      const s = await invoke<GlobalSettings>("get_global_settings");
      settings.value = { ...DEFAULT_SETTINGS, ...s };
      return true;
    } catch (e) {
      console.error("Failed to save global settings:", e);
      try {
        const s = await invoke<GlobalSettings>("get_global_settings");
        settings.value = { ...DEFAULT_SETTINGS, ...s };
      } catch (reloadErr) {
        console.error("Failed to reload global settings after save error:", reloadErr);
      }
      return false;
    } finally {
      saving.value = false;
    }
  }

  async function patch(partial: Partial<GlobalSettings>): Promise<boolean> {
    settings.value = { ...settings.value, ...partial };
    return save();
  }

  return {
    settings,
    loaded,
    saving,
    hideDevMenus,
    load,
    save,
    patch,
  };
});
