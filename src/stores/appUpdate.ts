import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { AppUpdateDownloadProgress, AppUpdateInfo } from "../types";
import {
  APP_UPDATE_AUTO_OPEN_DELAY_MS,
  shouldAutoOpenForSession as sessionAllowsAutoOpen,
  shouldAutoOpenModal,
  shouldOpenModalFromManualCheck,
  shouldShowPassivePrompt as passivePromptVisible,
} from "./appUpdateLogic";

const DISMISS_KEY = "app-update-dismissed";

export type DownloadPhase = "idle" | "downloading" | "complete" | "error";

function dismissedVersion(): string | null {
  try {
    return sessionStorage.getItem(DISMISS_KEY);
  } catch {
    return null;
  }
}

function markDismissed(version: string) {
  try {
    sessionStorage.setItem(DISMISS_KEY, version);
  } catch {
    /* ignore */
  }
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function normalizeUpdateInfo(info: AppUpdateInfo): AppUpdateInfo {
  const promptSuppressed = info.promptSuppressed ?? info.ignored ?? false;
  return {
    ...info,
    promptSuppressed,
    ignored: promptSuppressed,
  };
}

export const useAppUpdateStore = defineStore("appUpdate", () => {
  const updateInfo = ref<AppUpdateInfo | null>(null);
  const showModal = ref(false);
  const downloadPhase = ref<DownloadPhase>("idle");
  const downloadProgress = ref<AppUpdateDownloadProgress | null>(null);
  const downloadMessage = ref("");

  let unlistenUpdate: UnlistenFn | null = null;
  let unlistenProgress: UnlistenFn | null = null;
  let unlistenComplete: UnlistenFn | null = null;
  let unlistenError: UnlistenFn | null = null;
  let initialized = false;
  let autoOpenTimer: ReturnType<typeof setTimeout> | null = null;

  const isDownloading = computed(() => downloadPhase.value === "downloading");

  const shouldShowPassivePrompt = computed(() => passivePromptVisible(updateInfo.value));

  const progressLabel = computed(() => {
    const p = downloadProgress.value;
    if (!p) return "准备下载…";
    const downloaded = formatBytes(p.downloaded);
    if (p.total && p.total > 0) {
      return `${downloaded} / ${formatBytes(p.total)}${p.percent != null ? `（${Math.round(p.percent)}%）` : ""}`;
    }
    return `已下载 ${downloaded}`;
  });

  function clearAutoOpenTimer() {
    if (autoOpenTimer != null) {
      clearTimeout(autoOpenTimer);
      autoOpenTimer = null;
    }
  }

  function resetDownloadState() {
    downloadPhase.value = "idle";
    downloadProgress.value = null;
    downloadMessage.value = "";
  }

  function applyUpdateInfo(info: AppUpdateInfo | null) {
    if (!info) return;
    const normalized = normalizeUpdateInfo(info);
    if (normalized.updateAvailable) {
      const prev = updateInfo.value;
      // 启动检测晚到的旧 payload 可能仍带 promptSuppressed=false；
      // 不要覆盖本会话已忽略成功后的抑制态。
      if (
        prev?.latestVersion === normalized.latestVersion &&
        prev.promptSuppressed &&
        !normalized.promptSuppressed
      ) {
        normalized.promptSuppressed = true;
        normalized.ignored = true;
      }
      if (updateInfo.value?.latestVersion !== normalized.latestVersion) {
        resetDownloadState();
      }
      updateInfo.value = normalized;
      return;
    }
    if (normalized.checked) {
      updateInfo.value = null;
      showModal.value = false;
      resetDownloadState();
    }
  }

  function sessionAllowsOpen(version: string): boolean {
    return sessionAllowsAutoOpen(version, dismissedVersion());
  }

  function scheduleAutoOpen(version: string) {
    clearAutoOpenTimer();
    autoOpenTimer = setTimeout(() => {
      autoOpenTimer = null;
      const cur = updateInfo.value;
      if (
        cur &&
        cur.latestVersion === version &&
        shouldAutoOpenModal(cur) &&
        sessionAllowsOpen(cur.latestVersion)
      ) {
        showModal.value = true;
      }
    }, APP_UPDATE_AUTO_OPEN_DELAY_MS);
  }

  function onUpdateAvailable(info: AppUpdateInfo, autoOpen = false) {
    applyUpdateInfo(info);
    const normalized = updateInfo.value;
    if (
      autoOpen &&
      normalized &&
      shouldAutoOpenModal(normalized) &&
      sessionAllowsOpen(normalized.latestVersion)
    ) {
      // 推迟弹窗，避免启动早期挡住桥接初始化与语音键操作
      scheduleAutoOpen(normalized.latestVersion);
    }
  }

  function openModal(force = false) {
    const info = updateInfo.value;
    if (!info) return;
    clearAutoOpenTimer();
    if (force) {
      if (shouldOpenModalFromManualCheck(info)) showModal.value = true;
      return;
    }
    if (passivePromptVisible(info)) showModal.value = true;
  }

  function closeModal() {
    if (isDownloading.value) return;
    clearAutoOpenTimer();
    showModal.value = false;
    const ver = updateInfo.value?.latestVersion;
    if (ver) markDismissed(ver);
  }

  async function openUpdateLink(kind: "gitee" | "github") {
    const info = updateInfo.value;
    if (!info) return;
    const url = kind === "gitee" ? info.giteePage : info.githubPage;
    if (!url) return;
    try {
      await openUrl(url);
    } catch (e) {
      console.warn("open update url failed:", e);
      window.open(url, "_blank");
    }
  }

  async function startDownload() {
    const info = updateInfo.value;
    if (!info?.setupUrl || isDownloading.value) return;

    downloadPhase.value = "downloading";
    downloadProgress.value = { downloaded: 0, total: null, percent: null };
    downloadMessage.value = "";

    try {
      await invoke("download_app_update", {
        url: info.setupUrl,
        version: info.latestVersion,
      });
    } catch (e) {
      downloadPhase.value = "error";
      downloadMessage.value = String(e);
    }
  }

  async function ignoreCurrentUpdate() {
    const ver = updateInfo.value?.latestVersion;
    if (!ver || isDownloading.value) return;
    // 先关窗 + session dismiss：否则启动检测晚到的 app-update-available
    // 会再次 autoOpen，表现为「不再提醒没反应」。
    clearAutoOpenTimer();
    markDismissed(ver);
    showModal.value = false;
    try {
      const result = await invoke<AppUpdateInfo>("ignore_app_update", { version: ver });
      applyUpdateInfo(result);
      resetDownloadState();
    } catch (e) {
      console.warn("ignore_app_update failed:", e);
    }
  }

  async function checkForUpdate(force = false) {
    return invoke<AppUpdateInfo>("check_app_update", { force });
  }

  async function init() {
    if (initialized) return;
    initialized = true;

    try {
      unlistenUpdate = await listen<AppUpdateInfo>("app-update-available", (event) => {
        if (event.payload) {
          onUpdateAvailable(event.payload, true);
        }
      });
    } catch (e) {
      console.warn("listen app-update-available failed:", e);
    }

    try {
      unlistenProgress = await listen<AppUpdateDownloadProgress>(
        "app-update-download-progress",
        (event) => {
          if (!event.payload) return;
          downloadPhase.value = "downloading";
          downloadProgress.value = event.payload;
        },
      );
    } catch (e) {
      console.warn("listen app-update-download-progress failed:", e);
    }

    try {
      unlistenComplete = await listen<{ path: string }>("app-update-download-complete", () => {
        downloadPhase.value = "complete";
        downloadMessage.value =
          "已开始静默升级：本程序即将退出，随后显示升级进度窗（卸旧装新、保留配置），完成后自动打开新版。";
      });
    } catch (e) {
      console.warn("listen app-update-download-complete failed:", e);
    }

    try {
      unlistenError = await listen<{ message: string }>("app-update-download-error", (event) => {
        downloadPhase.value = "error";
        downloadMessage.value = event.payload?.message || "下载失败";
      });
    } catch (e) {
      console.warn("listen app-update-download-error failed:", e);
    }

    try {
      const cached = await invoke<AppUpdateInfo>("get_app_update_state");
      if (cached.updateAvailable) {
        onUpdateAvailable(cached, true);
      }
    } catch {
      /* ignore */
    }
  }

  function dispose() {
    clearAutoOpenTimer();
    unlistenUpdate?.();
    unlistenProgress?.();
    unlistenComplete?.();
    unlistenError?.();
    unlistenUpdate = null;
    unlistenProgress = null;
    unlistenComplete = null;
    unlistenError = null;
    initialized = false;
  }

  return {
    updateInfo,
    showModal,
    downloadPhase,
    downloadProgress,
    downloadMessage,
    isDownloading,
    shouldShowPassivePrompt,
    progressLabel,
    applyUpdateInfo,
    onUpdateAvailable,
    openModal,
    closeModal,
    openUpdateLink,
    startDownload,
    ignoreCurrentUpdate,
    checkForUpdate,
    init,
    dispose,
  };
});
