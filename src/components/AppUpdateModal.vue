<script setup lang="ts">
import { storeToRefs } from "pinia";
import { useAppUpdateStore } from "../stores/appUpdate";

const store = useAppUpdateStore();
const {
  updateInfo,
  showModal,
  downloadPhase,
  downloadProgress,
  downloadMessage,
  isDownloading,
  progressLabel,
} = storeToRefs(store);

function progressWidth(): string {
  const p = downloadProgress.value;
  if (p?.percent != null) return `${Math.min(100, Math.max(0, p.percent))}%`;
  if (downloadPhase.value === "complete") return "100%";
  return "0%";
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="showModal && updateInfo?.updateAvailable"
      class="update-backdrop"
      role="presentation"
      @click.self="store.closeModal()"
    >
      <div
        class="update-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="app-update-title"
      >
        <div class="update-dialog-scroll">
          <h3 id="app-update-title">发现新版本 V{{ updateInfo.latestVersion }}</h3>
          <p class="update-intro">
            当前版本 V{{ updateInfo.currentVersion }}。可在软件内下载安装包，完成后自动启动安装程序（安装前请先退出本软件）。
          </p>
          <section v-if="updateInfo.notes" class="update-notes-block" aria-label="更新内容">
            <h4 class="update-notes-heading">更新内容</h4>
            <p class="update-notes">{{ updateInfo.notes }}</p>
          </section>
        </div>

        <div
          v-if="downloadPhase !== 'idle'"
          class="update-progress-block"
          role="status"
          aria-live="polite"
        >
          <div class="update-progress-head">
            <span class="update-progress-label">
              {{
                downloadPhase === "downloading"
                  ? "正在下载…"
                  : downloadPhase === "complete"
                    ? "下载完成"
                    : "下载失败"
              }}
            </span>
            <span v-if="downloadPhase === 'downloading'" class="update-progress-meta">
              {{ progressLabel }}
            </span>
          </div>
          <div
            class="update-progress-track"
            :class="{ indeterminate: downloadPhase === 'downloading' && downloadProgress?.percent == null }"
          >
            <div
              class="update-progress-bar"
              :style="{ width: progressWidth() }"
            />
          </div>
          <p v-if="downloadMessage" class="update-progress-msg">{{ downloadMessage }}</p>
        </div>

        <div class="update-actions">
          <button
            class="update-btn update-btn-primary"
            type="button"
            :disabled="isDownloading || downloadPhase === 'complete'"
            @click="store.startDownload()"
          >
            {{ isDownloading ? "下载中…" : downloadPhase === "complete" ? "已启动安装" : "下载并安装" }}
          </button>
          <button
            class="update-btn update-btn-secondary"
            type="button"
            :disabled="isDownloading"
            @click="store.openUpdateLink('gitee')"
          >
            去 Gitee 下载
          </button>
          <button
            class="update-btn update-btn-secondary"
            type="button"
            :disabled="isDownloading"
            @click="store.openUpdateLink('github')"
          >
            去 GitHub 下载
          </button>
          <button
            class="update-btn update-btn-secondary"
            type="button"
            :disabled="isDownloading"
            @click="store.ignoreCurrentUpdate()"
          >
            忽略此版本
          </button>
          <button
            class="update-btn update-btn-secondary"
            type="button"
            :disabled="isDownloading"
            @click="store.closeModal()"
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.update-backdrop {
  position: fixed;
  inset: 0;
  z-index: 4500;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
  background: rgba(15, 23, 42, 0.45);
}

.update-dialog {
  display: flex;
  flex-direction: column;
  width: min(480px, 100%);
  max-height: min(85vh, 640px);
  padding: 18px 18px 14px;
  border-radius: 10px;
  background: #fff;
  box-shadow: 0 12px 40px rgba(15, 23, 42, 0.25);
  color: var(--text, #1e293b);
  overflow: hidden;
}

.update-dialog-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.update-dialog h3 {
  margin: 0 0 8px;
  font-size: 16px;
  font-weight: 600;
}

.update-intro {
  margin: 0 0 12px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-secondary, #64748b);
}

.update-notes-block {
  margin: 0 0 4px;
  padding: 10px 12px;
  border: 1px solid var(--border, #e2e8f0);
  border-radius: 8px;
  background: #f8fafc;
  max-height: calc(13px * 1.55 * 8 + 28px);
  overflow-y: auto;
}

.update-notes-heading {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text, #1e293b);
}

.update-notes {
  margin: 0;
  font-size: 13px;
  line-height: 1.55;
  color: var(--text-secondary, #64748b);
  white-space: pre-line;
}

.update-progress-block {
  flex-shrink: 0;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border, #e2e8f0);
}

.update-progress-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 6px;
}

.update-progress-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text, #1e293b);
}

.update-progress-meta {
  font-size: 11px;
  color: var(--text-secondary, #64748b);
  white-space: nowrap;
}

.update-progress-track {
  height: 8px;
  border-radius: 999px;
  background: #e2e8f0;
  overflow: hidden;
}

.update-progress-track.indeterminate .update-progress-bar {
  width: 35% !important;
  animation: update-progress-indeterminate 1.2s ease-in-out infinite;
}

.update-progress-bar {
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #3b82f6, #1d4ed8);
  transition: width 0.15s ease;
}

.update-progress-msg {
  margin: 8px 0 0;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-secondary, #64748b);
}

.update-actions {
  flex-shrink: 0;
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border, #e2e8f0);
}

.update-btn {
  height: 32px;
  padding: 0 14px;
  border-radius: 6px;
  border: 1px solid transparent;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}

.update-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.update-btn-primary {
  background: var(--primary, #1a73e8);
  color: #fff;
}

.update-btn-primary:hover:not(:disabled) {
  filter: brightness(0.95);
}

.update-btn-secondary {
  background: #fff;
  border-color: var(--border, #e2e8f0);
  color: var(--text, #1e293b);
}

.update-btn-secondary:hover:not(:disabled) {
  background: #f1f5f9;
}

@keyframes update-progress-indeterminate {
  0% {
    transform: translateX(-120%);
  }
  100% {
    transform: translateX(320%);
  }
}
</style>
