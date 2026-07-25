<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DeviceConfig, BridgeType, KeyAction } from "../types";

const props = defineProps<{
  bridgeType: BridgeType;
  config: DeviceConfig;
  focusButtonId?: string | null;
}>();

const emit = defineEmits<{
  save: [config: DeviceConfig];
}>();

const editingKey = ref<string | null>(null);
const capturing = ref(false);
const captureError = ref<string | null>(null);
const captureStatus = ref("先点「录入」，再按目标单键或组合键");
const liveLabels = ref<string[]>([]);
const listRef = ref<HTMLElement | null>(null);

let unlistenCaptured: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let applied = false;

const PRIMARY_IDS = [
  "power",
  "mic",
  "up",
  "left",
  "ok",
  "right",
  "down",
  "back",
  "volume_up",
  "home",
  "volume_down",
  "menu",
  "tv",
];

const buttons = computed(() => {
  const aliases = props.config?.button_aliases || {};
  const ids = PRIMARY_IDS.filter((id) => aliases[id] || props.config.button_bindings?.[id]);
  const extra = Object.keys(aliases).filter((id) => !PRIMARY_IDS.includes(id));
  return [...ids, ...extra].map((id) => ({
    id,
    label: aliases[id] || id,
    action: props.config.button_bindings?.[id] || { type: "None", value: null },
  }));
});

watch(
  () => props.focusButtonId,
  async (id) => {
    if (!id) return;
    editingKey.value = id;
    await nextTick();
    const el = listRef.value?.querySelector(`[data-button-id="${id}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }
);

onMounted(async () => {
  try {
    unlistenCaptured = await listen<{ keys: number[]; labels: string[] }>(
      "shortcut-captured",
      (event) => {
        const keys = event.payload?.keys;
        if (!keys?.length) return;
        onCaptured(keys, event.payload.labels || []);
      }
    );
    unlistenProgress = await listen<{ labels: string[] }>(
      "shortcut-capture-progress",
      (event) => {
        liveLabels.value = event.payload?.labels || [];
        if (capturing.value && liveLabels.value.length) {
          captureStatus.value = `正在录入：${liveLabels.value.join(" + ")} …`;
        }
      }
    );
  } catch (e) {
    console.warn("shortcut listen failed", e);
  }
});

onUnmounted(() => {
  stopPolling();
  unlistenCaptured?.();
  unlistenProgress?.();
  if (capturing.value) {
    invoke("capture_shortcut_stop").catch(() => {});
  }
});

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function startPolling() {
  stopPolling();
  applied = false;
  pollTimer = setInterval(async () => {
    if (!capturing.value || applied) return;
    try {
      const result = await invoke<{ keys: number[]; labels: string[] } | null>(
        "capture_shortcut_poll"
      );
      if (result && Array.isArray(result.keys) && result.keys.length > 0) {
        onCaptured(result.keys, result.labels || []);
      }
    } catch (e) {
      console.warn("capture poll failed", e);
    }
  }, 50);
}

function onCaptured(keys: number[], labels: string[]) {
  if (applied) return;
  const buttonId = editingKey.value;
  if (!buttonId) return;
  applied = true;
  stopPolling();

  applyCapturedKeys(buttonId, keys);
  capturing.value = false;
  editingKey.value = null;
  liveLabels.value = [];
  captureStatus.value = `已录入 ${labels.join(" + ") || keys.map(vkName).join(" + ")}，已保存`;

  // 停止后台轮询线程（结果已取出，不怕 clear）
  invoke("capture_shortcut_stop").catch(() => {});
}

async function startEdit(buttonId: string) {
  if (capturing.value) {
    await cancelCapture();
    return;
  }

  captureError.value = null;
  editingKey.value = buttonId;
  capturing.value = true;
  liveLabels.value = [];
  applied = false;
  captureStatus.value = "正在录入：请按目标键或组合键……";
  try {
    await invoke("capture_shortcut_start");
    startPolling();
  } catch (e) {
    capturing.value = false;
    editingKey.value = null;
    stopPolling();
    captureError.value = String(e);
    captureStatus.value = "录入失败，可以重试";
  }
}

async function cancelCapture() {
  stopPolling();
  capturing.value = false;
  editingKey.value = null;
  liveLabels.value = [];
  applied = false;
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
  captureStatus.value = "已取消录入";
}

function applyCapturedKeys(buttonId: string, vks: number[]) {
  let action: KeyAction;
  if (!vks.length) {
    action = { type: "None", value: null };
  } else if (vks.length === 1) {
    action = { type: "SingleKey", value: vks[0] };
  } else {
    action = { type: "ComboKey", value: [...vks] };
  }
  if (!props.config.button_bindings) {
    (props.config as DeviceConfig).button_bindings = {};
  }
  props.config.button_bindings[buttonId] = action;
  // 对齐 Python：mic 映射同步到 voice / voice_hotkey
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
  };
  if (buttonId === "mic" || buttonId === "voice") {
    next.button_bindings.mic = action;
    next.button_bindings.voice = action;
    next.voice_hotkey = vksToHotkeyNames(vks);
  }
  emit("save", next);
}

function clearBinding(buttonId: string) {
  props.config.button_bindings[buttonId] = { type: "None", value: null };
  const next: DeviceConfig = {
    ...props.config,
    button_bindings: { ...props.config.button_bindings },
  };
  if (buttonId === "mic" || buttonId === "voice") {
    next.button_bindings.mic = { type: "None", value: null };
    next.button_bindings.voice = { type: "None", value: null };
    next.voice_hotkey = [];
  }
  emit("save", next);
  captureStatus.value = "已清除绑定";
}

function vksToHotkeyNames(vks: number[]): string[] {
  const map: Record<number, string> = {
    0xa2: "leftctrl",
    0xa3: "rightctrl",
    0x11: "ctrl",
    0xa0: "leftshift",
    0xa1: "rightshift",
    0x10: "shift",
    0xa4: "leftalt",
    0xa5: "rightalt",
    0x12: "alt",
    0x5b: "leftwin",
    0x5c: "rightwin",
    0x20: "space",
    0x0d: "enter",
  };
  return vks.map((vk) => {
    if (map[vk]) return map[vk];
    if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk).toLowerCase();
    if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
    if (vk >= 0x70 && vk <= 0x7b) return `f${vk - 0x6f}`;
    return `vk_${vk.toString(16)}`;
  });
}

function actionLabel(action: KeyAction): string {
  if (!action || action.type === "None") return "未绑定";
  if (action.type === "SingleKey") return vkName(Number(action.value));
  if (action.type === "ComboKey") {
    const arr = Array.isArray(action.value) ? action.value : [];
    return arr.map((v) => vkName(Number(v))).join(" + ");
  }
  if (action.type === "TextInput") return `文字: ${action.value}`;
  if (action.type === "LaunchApp") return `启动: ${action.value}`;
  return "—";
}

function vkName(vk: number): string {
  const map: Record<number, string> = {
    0x08: "Backspace",
    0x09: "Tab",
    0x0d: "Enter",
    0x1b: "Esc",
    0x20: "Space",
    0x21: "PageUp",
    0x22: "PageDown",
    0x23: "End",
    0x24: "Home",
    0x25: "←",
    0x26: "↑",
    0x27: "→",
    0x28: "↓",
    0x2d: "Insert",
    0x2e: "Delete",
    0x10: "Shift",
    0xa0: "Shift",
    0xa1: "右 Shift",
    0x11: "Ctrl",
    0xa2: "Ctrl",
    0xa3: "右 Ctrl",
    0x12: "Alt",
    0xa4: "Alt",
    0xa5: "右 Alt",
    0x5b: "左 Win",
    0x5c: "右 Win",
    0xaf: "Vol+",
    0xae: "Vol-",
    0xad: "Mute",
  };
  if (map[vk]) return map[vk];
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
  if (vk >= 0x70 && vk <= 0x7b) return `F${vk - 0x6f}`;
  return `VK_0x${vk.toString(16).toUpperCase()}`;
}
</script>

<template>
  <div class="key-editor">
    <p class="capture-hint">{{ captureStatus }}</p>
    <p v-if="captureError" class="capture-error">{{ captureError }}</p>
    <p v-if="!buttons.length" class="capture-error">
      没有可映射的按键（button_aliases 为空）
    </p>

    <div class="key-list" ref="listRef">
      <div
        v-for="btn in buttons"
        :key="btn.id"
        :data-button-id="btn.id"
        :class="['key-row', { editing: editingKey === btn.id }]"
      >
        <span class="key-name">{{ btn.label }}</span>
        <div class="key-action-area">
          <span :class="['key-action', { unbound: btn.action.type === 'None' }]">
            {{ actionLabel(btn.action) }}
          </span>
          <div class="key-actions">
            <button
              class="btn-sm btn-edit"
              @click="startEdit(btn.id)"
              :disabled="capturing && editingKey !== btn.id"
            >
              {{
                editingKey === btn.id && capturing
                  ? "取消录入"
                  : "按真实键盘录入"
              }}
            </button>
            <button
              v-if="btn.action.type !== 'None'"
              class="btn-sm btn-clear"
              @click="clearBinding(btn.id)"
              :disabled="capturing"
            >
              ✕
            </button>
          </div>
        </div>
      </div>
    </div>

    <div v-if="capturing" class="capture-overlay">
      <div class="capture-box">
        <p class="capture-title">正在录入</p>
        <p class="capture-live">
          {{
            liveLabels.length
              ? liveLabels.join(" + ") + " …"
              : "请按目标键或组合键"
          }}
        </p>
        <p class="capture-note">松开后自动完成并保存</p>
        <button class="btn btn-primary" @click="cancelCapture">取消录入</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.key-editor {
  position: relative;
}

.capture-hint {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 10px;
}

.capture-error {
  color: var(--danger);
  font-size: 12px;
  margin-bottom: 8px;
}

.key-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.key-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-radius: 6px;
  border: 1px solid transparent;
  transition: all 0.15s ease;
}

.key-row:hover {
  background: #f8fafc;
  border-color: var(--border);
}

.key-row.editing {
  background: #eff6ff;
  border-color: var(--primary);
}

.key-name {
  font-size: 13px;
  font-weight: 500;
  min-width: 80px;
}

.key-action-area {
  display: flex;
  align-items: center;
  gap: 8px;
}

.key-action {
  font-size: 12px;
  font-family: monospace;
  background: #f1f5f9;
  padding: 3px 8px;
  border-radius: 4px;
  color: var(--text);
}

.key-action.unbound {
  color: var(--text-secondary);
  background: transparent;
}

.key-actions {
  display: flex;
  gap: 4px;
}

.btn-sm {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  background: var(--card-bg);
  color: var(--text);
  transition: all 0.15s ease;
}

.btn-sm:hover {
  background: #f1f5f9;
}

.btn-edit {
  color: var(--primary);
  border-color: var(--primary);
}
.btn-edit:hover:not(:disabled) {
  background: #eff6ff;
}
.btn-edit:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-clear {
  color: var(--danger);
  border-color: transparent;
}
.btn-clear:hover:not(:disabled) {
  background: #fef2f2;
  border-color: var(--danger);
}

.capture-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.capture-box {
  background: var(--card-bg);
  padding: 32px 40px;
  border-radius: 12px;
  text-align: center;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.2);
  min-width: 320px;
}

.capture-title {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 8px;
}

.capture-live {
  font-size: 22px;
  font-family: monospace;
  font-weight: 500;
  color: var(--primary);
  min-height: 32px;
  margin-bottom: 8px;
}

.capture-note {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 20px;
}
</style>
