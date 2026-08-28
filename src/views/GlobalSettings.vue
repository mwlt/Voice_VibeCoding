<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { GlobalSettings } from "../types";
import { useAppUpdateStore } from "../stores/appUpdate";
import sanodiaLogo from "../assets/mwlt_sanodia_logo.png";

const appUpdate = useAppUpdateStore();

const settings = ref<GlobalSettings>({
  autostart: false,
  language: "zh-CN",
  minimize_to_tray: true,
  start_minimized_to_tray: false,
});

const saved = ref(true);
const saving = ref(false);
const updateChecking = ref(false);
const updateHint = ref("");

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

async function openExternal(url: string) {
  try {
    await openUrl(url);
  } catch (e) {
    console.warn("open url failed:", e);
    window.open(url, "_blank");
  }
}

async function checkUpdate() {
  updateChecking.value = true;
  updateHint.value = "正在检查…";
  try {
    const result = await appUpdate.checkForUpdate(true);
    appUpdate.applyUpdateInfo(result);
    if (result.error) {
      updateHint.value = `检查失败：${result.error}`;
    } else if (result.updateAvailable) {
      if (result.promptSuppressed ?? result.ignored) {
        updateHint.value = `发现新版本 V${result.latestVersion}（已关闭自动提醒，仍可在此更新）。`;
      } else {
        updateHint.value = `发现新版本 V${result.latestVersion}。`;
      }
      appUpdate.openModal(true);
    } else {
      updateHint.value = `已是最新（V${result.currentVersion}）。`;
    }
  } catch (e) {
    updateHint.value = `检查失败：${e}`;
  } finally {
    updateChecking.value = false;
  }
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="header-left">
        <h2>⚙️ 全局设置</h2>
        <button
          class="btn btn-secondary"
          type="button"
          :disabled="updateChecking"
          @click="checkUpdate"
        >
          {{ updateChecking ? "检查中…" : "检查更新" }}
        </button>
        <span v-if="updateHint" class="header-update-hint">{{ updateHint }}</span>
      </div>
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
            <span class="setting-label">启动后最小化到托盘</span>
            <span class="setting-desc"
              >启动后直接进托盘（不占任务栏），点托盘图标打开</span
            >
          </div>
          <label class="toggle">
            <input
              type="checkbox"
              v-model="settings.start_minimized_to_tray"
              @change="onSettingChange"
            />
            <span class="toggle-slider"></span>
          </label>
        </div>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">最小化到托盘</span>
            <span class="setting-desc"
              >点关闭按钮时进托盘（不占任务栏，可点托盘再打开）。关闭此项后，关窗即退出软件</span
            >
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

      <section class="card credit-card">
        <h3>版本信息</h3>
        <div class="credit-layout">
          <div class="credit-logo-wrap">
            <img
              class="credit-logo"
              :src="sanodiaLogo"
              alt="Sanodia / mwlt"
            />
          </div>
          <div class="credit-columns">
            <div class="credit-row">
              <div class="credit-col">
                <p class="credit-lead">
                  本软件 : Rust+tauri2+vue3 Windows版（基于 Python 版本重构）
                </p>
                <p class="credit-author">作者：mwlt</p>
                <div class="credit-block">
                  <span class="credit-k">Gitee</span>
                  <button
                    type="button"
                    class="credit-link"
                    @click="openExternal('https://gitee.com/mwlt/remote-voice-vibe-coding')"
                  >
                    https://gitee.com/mwlt/remote-voice-vibe-coding
                  </button>
                </div>
                <div class="credit-block">
                  <span class="credit-k">GitHub</span>
                  <button
                    type="button"
                    class="credit-link"
                    @click="openExternal('https://github.com/mwlt/Voice_VibeCoding')"
                  >
                    https://github.com/mwlt/Voice_VibeCoding
                  </button>
                </div>
              </div>
              <div class="credit-col">
                <p class="credit-lead">Python Windows 版</p>
                <p class="credit-author">作者：xxb26553663-star</p>
                <div class="credit-block">
                  <span class="credit-k">GitHub</span>
                  <button
                    type="button"
                    class="credit-link"
                    @click="openExternal('https://github.com/xxb26553663-star/remote-bridge-hub')"
                  >
                    https://github.com/xxb26553663-star/remote-bridge-hub
                  </button>
                </div>
              </div>
            </div>
            <div class="credit-row credit-row-divider">
              <div class="credit-col">
                <p class="credit-lead">Apple macOS 版</p>
                <p class="credit-author">作者：nijez</p>
                <div class="credit-block">
                  <span class="credit-k">GitHub</span>
                  <button
                    type="button"
                    class="credit-link"
                    @click="openExternal('https://github.com/nijez/open-voice-bridge')"
                  >
                    https://github.com/nijez/open-voice-bridge
                  </button>
                </div>
              </div>
              <div class="credit-col">
                <p class="credit-lead">Rust 语言 Windows 版</p>
                <p class="credit-author">作者：LightyearXizIl</p>
                <div class="credit-block">
                  <span class="credit-k">GitHub</span>
                  <button
                    type="button"
                    class="credit-link"
                    @click="openExternal('https://github.com/LightyearXizIl/Nexus-Prime')"
                  >
                    https://github.com/LightyearXizIl/Nexus-Prime
                  </button>
                </div>
              </div>
            </div>
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
  gap: 12px;
  margin-bottom: 20px;
}
.page-header h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  white-space: nowrap;
}
.header-left {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  flex: 1;
}
.header-update-hint {
  min-width: 0;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.45;
}
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

.credit-layout {
  display: flex;
  align-items: flex-start;
  gap: 20px;
}

.credit-logo-wrap {
  flex-shrink: 0;
  width: 120px;
  padding: 10px;
  border-radius: 10px;
  background: #fff;
  border: 1px solid var(--border);
}

.credit-logo {
  display: block;
  width: 100%;
  height: auto;
  object-fit: contain;
}

.credit-columns {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

/* 一行两列：本软件 | Python（第一行）、macOS | Rust Windows 版（第二行） */
.credit-row {
  display: grid;
  grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr);
  gap: 20px;
}

/* 行间横线分隔（空间不足时新增版本放下一行的视觉分隔） */
.credit-row-divider {
  border-top: 1px solid var(--border);
  padding-top: 14px;
}

.credit-col {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  padding-left: 16px;
  border-left: 1px solid var(--border);
}

.credit-col:first-child {
  padding-left: 0;
  border-left: none;
}

.credit-lead {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.45;
  color: var(--text);
}

.credit-author {
  margin: 0 0 4px;
  font-size: 13px;
  color: var(--text-secondary);
}

.credit-block {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.credit-k {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}

.credit-link {
  align-self: flex-start;
  max-width: 100%;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--primary);
  font-size: 12px;
  line-height: 1.45;
  text-align: left;
  word-break: break-all;
  cursor: pointer;
}

.credit-link:hover {
  text-decoration: underline;
}

@media (max-width: 900px) {
  .credit-row {
    grid-template-columns: 1fr;
    gap: 0;
  }
  /* 移动端单列：行间仍保留横线分隔，行内各项用上边框区分 */
  .credit-row-divider {
    margin-top: 14px;
  }
  .credit-col {
    padding-left: 0;
    border-left: none;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
  .credit-col:first-child {
    padding-top: 0;
    border-top: none;
  }
}

@media (max-width: 720px) {
  .credit-layout {
    flex-direction: column;
    align-items: stretch;
  }
  .credit-logo-wrap {
    width: 100px;
  }
}
</style>
