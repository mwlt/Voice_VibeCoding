<script setup lang="ts">
import { ref, onMounted } from "vue";
import { storeToRefs } from "pinia";
import { useRoute, useRouter } from "vue-router";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useAppUpdateStore } from "../stores/appUpdate";
import { useGlobalSettingsStore } from "../stores/globalSettings";
import sanodiaLogo from "../assets/mwlt_sanodia_logo.png";

const appUpdate = useAppUpdateStore();
const globalSettings = useGlobalSettingsStore();
const { settings } = storeToRefs(globalSettings);
const route = useRoute();
const router = useRouter();

const updateChecking = ref(false);
const updateHint = ref("");
const toastVisible = ref(false);
let toastTimer: ReturnType<typeof setTimeout> | null = null;

onMounted(async () => {
  if (!globalSettings.loaded) {
    await globalSettings.load();
  }
});

function showSavedToast() {
  toastVisible.value = true;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastVisible.value = false;
    toastTimer = null;
  }, 2000);
}

async function onSettingChange() {
  const ok = await globalSettings.save();
  if (ok) {
    showSavedToast();
    if (
      settings.value.hide_dev_menus &&
      (route.path === "/t1" || route.path === "/v60")
    ) {
      router.push("/xiaomi");
    }
  } else {
    updateHint.value = "设置保存失败，请重试";
  }
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
    </header>

    <div class="page-body">
      <section class="card">
        <h3>通用</h3>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">开机自启</span>
            <span class="setting-desc"
              >Windows 登录时自动运行（与下方「启动后最小化到托盘」独立；是否进托盘由该选项决定）</span
            >
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
              >关：手动打开/开机自启/再点图标 → 显示窗口。开：上述情况均进托盘（无窗口、无任务栏）；点托盘图标可打开。仅对下次启动生效，不改变当前窗口状态</span
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
            <span class="setting-label">隐藏开发中项目菜单</span>
            <span class="setting-desc"
              >开启后隐藏顶部 T1、V60 菜单；关闭则显示</span
            >
          </div>
          <label class="toggle">
            <input
              type="checkbox"
              v-model="settings.hide_dev_menus"
              @change="onSettingChange"
            />
            <span class="toggle-slider"></span>
          </label>
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

    <Teleport to="body">
      <div v-if="toastVisible" class="settings-toast" role="status">
        设置已保存
      </div>
    </Teleport>
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
  gap: 16px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
}
.setting-row:last-child {
  border-bottom: none;
  padding-bottom: 0;
}
.setting-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.setting-label {
  font-size: 14px;
  font-weight: 500;
  color: var(--text);
}
.setting-desc {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.45;
}

.toggle {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
  flex-shrink: 0;
}
.toggle input {
  opacity: 0;
  width: 0;
  height: 0;
}
.toggle-slider {
  position: absolute;
  cursor: pointer;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: #cbd5e1;
  transition: 0.2s;
  border-radius: 24px;
}
.toggle-slider:before {
  position: absolute;
  content: "";
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  transition: 0.2s;
  border-radius: 50%;
}
.toggle input:checked + .toggle-slider {
  background-color: var(--primary, #3b82f6);
}
.toggle input:checked + .toggle-slider:before {
  transform: translateX(20px);
}

.btn {
  height: 32px;
  padding: 0 14px;
  border-radius: 6px;
  border: 1px solid transparent;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  white-space: nowrap;
}
.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.btn-primary {
  background: var(--primary, #3b82f6);
  color: #fff;
}
.btn-secondary {
  background: #fff;
  border-color: var(--border, #e2e8f0);
  color: var(--text, #1e293b);
}
.btn-secondary:hover:not(:disabled) {
  background: #f8fafc;
}

.credit-card h3 {
  margin-bottom: 12px;
}
.credit-layout {
  display: flex;
  gap: 20px;
  align-items: flex-start;
}
.credit-logo-wrap {
  flex-shrink: 0;
}
.credit-logo {
  width: 72px;
  height: auto;
  display: block;
}
.credit-columns {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.credit-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 20px;
}
.credit-row-divider {
  padding-top: 16px;
  border-top: 1px solid var(--border);
}
.credit-lead {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text);
}
.credit-author {
  margin: 0 0 8px;
  font-size: 12px;
  color: var(--text-secondary);
}
.credit-block {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-bottom: 6px;
}
.credit-k {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.credit-link {
  padding: 0;
  border: none;
  background: none;
  color: var(--primary, #3b82f6);
  font-size: 12px;
  text-align: left;
  cursor: pointer;
  word-break: break-all;
}
.credit-link:hover {
  text-decoration: underline;
}

.settings-toast {
  position: fixed;
  left: 50%;
  top: 60px;
  transform: translateX(-50%);
  z-index: 4000;
  padding: 10px 18px;
  border-radius: 8px;
  background: rgba(15, 23, 42, 0.92);
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.25);
  pointer-events: none;
  animation: settings-toast-in 0.2s ease-out;
}

@keyframes settings-toast-in {
  from {
    opacity: 0;
    transform: translateX(-50%) translateY(-8px);
  }
  to {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
}
</style>
