import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { AppUpdateDownloadProgress, AppUpdateInfo } from "../types";
import {
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

  function resetDownloadState() {
    downloadPhase.value = "idle";
    downloadProgress.value = null;
    downloadMessage.value = "";
  }

  function applyUpdateInfo(info: AppUpdateInfo | null) {
    if (!info) return;
    const normalized = normalizeUpdateInfo(info);
    if (normalized.updateAvailable) {
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

  function shouldAutoOpenForSession(version: string): boolean {
    return dismissedVersion() !== version;
  }

  function onUpdateAvailable(info: AppUpdateInfo, autoOpen = false) {
    applyUpdateInfo(info);
    const normalized = updateInfo.value;
    if (
      autoOpen &&
      normalized &&
      shouldAutoOpenModal(normalized) &&
      shouldAutoOpenForSession(normalized.latestVersion)
    ) {
      showModal.value = true;
    }
  }

  function openModal(force = false) {
    const info = updateInfo.value;
    if (!info) return;
    if (force) {
      if (shouldOpenModalFromManualCheck(info)) showModal.value = true;
      return;
    }
    if (passivePromptVisible(info)) showModal.value = true;
  }

  function closeModal() {
    if (isDownloading.value) return;
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
    try {
      const result = await invoke<AppUpdateInfo>("ignore_app_update", { version: ver });
      applyUpdateInfo(result);
      showModal.value = false;
      resetDownloadState();
    } catch (e) {
      console.warn("ignore_app_update failed:", e);
      throw e;
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
        downloadMessage.value = "安装程序已启动，请按提示完成安装（建议先退出本软件）。";
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
