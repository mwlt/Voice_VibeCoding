<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { GlobalSettings } from "../types";

const settings = ref<GlobalSettings>({
  autostart: false,
  language: "zh-CN",
  minimize_to_tray: true,
});

const saved = ref(true);
const saving = ref(false);

onMounted(async () => {
  try {
    const s = await invoke<GlobalSettings>("get_global_settings");
    settings.value = s;
  } catch (e) {
    console.error("Failed to load settings:", e);
  }
});

async function saveSettings() {
  saving.value = true;
  try {
    await invoke("save_global_settings", { settings: settings.value });
    saved.value = true;
  } catch (e) {
    console.error("Failed to save settings:", e);
  } finally {
    saving.value = false;
  }
}

function onSettingChange() {
  saved.value = false;
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h2>⚙️ 全局设置</h2>
      <button
        class="btn btn-primary"
        :disabled="saved || saving"
        @click="saveSettings"
      >
        {{ saving ? "保存中..." : saved ? "已保存" : "保存设置" }}
      </button>
    </header>

    <div class="page-body">
      <section class="card">
        <h3>通用</h3>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">开机自启</span>
            <span class="setting-desc">Windows 启动时自动运行 Voice VibeCoding</span>
          </div>
          <label class="toggle">
            <input
              type="checkbox"
              v-model="settings.autostart"
              @change="onSettingChange"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">最小化到托盘</span>
            <span class="setting-desc">关闭窗口时最小化到系统托盘而非退出</span>
          </div>
          <label class="toggle">
            <input
              type="checkbox"
              v-model="settings.minimize_to_tray"
              @change="onSettingChange"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">界面语言</span>
            <span class="setting-desc">选择应用程序的显示语言</span>
          </div>
          <select
            v-model="settings.language"
            class="form-select"
            @change="onSettingChange"
          >
            <option value="zh-CN">简体中文</option>
            <option value="zh-TW">繁體中文</option>
            <option value="en">English</option>
          </select>
        </div>
      </section>

      <section class="card">
        <h3>关于</h3>
        <div class="about-grid">
          <div class="about-item">
            <span class="about-label">应用名称</span>
            <span class="about-value">Voice VibeCoding（语音氛围编程）</span>
          </div>
          <div class="about-item">
            <span class="about-label">版本</span>
            <span class="about-value">v1.3.3</span>
          </div>
          <div class="about-item">
            <span class="about-label">技术栈</span>
            <span class="about-value">Rust + Tauri 2 + Vue 3</span>
          </div>
          <div class="about-item">
            <span class="about-label">支持设备</span>
            <span class="about-value">小米遥控器 2 Pro / T1 遥控器 / 汉王 V60</span>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.page {
  width: 100%;
  max-width: none;
  box-sizing: border-box;
}
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
}
.page-header h2 { font-size: 20px; font-weight: 600; }
.page-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  width: 100%;
}

.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px;
}
.card h3 { font-size: 15px; font-weight: 600; margin-bottom: 16px; }

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
}
.setting-row:last-child { border-bottom: none; }

.setting-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.setting-label { font-size: 14px; font-weight: 500; }
.setting-desc { font-size: 12px; color: var(--text-secondary); }

.form-select {
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  font-size: 13px;
  background: var(--card-bg);
}

.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
  flex-shrink: 0;
}
.toggle input { display: none; }
.toggle-slider {
  position: absolute;
  inset: 0;
  background: #cbd5e1;
  border-radius: 12px;
  cursor: pointer;
  transition: background 0.2s ease;
}
.toggle-slider::before {
  content: "";
  position: absolute;
  width: 18px;
  height: 18px;
  left: 3px;
  top: 3px;
  background: white;
  border-radius: 50%;
  transition: transform 0.2s ease;
}
.toggle input:checked + .toggle-slider {
  background: var(--primary);
}
.toggle input:checked + .toggle-slider::before {
  transform: translateX(20px);
}

.btn {
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
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

.about-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.about-item { display: flex; flex-direction: column; gap: 4px; }
.about-label { font-size: 12px; color: var(--text-secondary); }
.about-value { font-size: 13px; font-weight: 500; }
</style>
