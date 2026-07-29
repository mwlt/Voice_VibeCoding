<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  ref,
  watch,
} from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DeviceConfig, KeyAction } from "../types";
import RemoteHotspot from "./RemoteHotspot.vue";
import { MEDIA_PICK_KEYS, vkDisplayName } from "../utils/vkDisplay";

const props = defineProps<{
  config: DeviceConfig;
}>();

const emit = defineEmits<{
  save: [config: DeviceConfig];
}>();

const LEFT_IDS = [
  "power",
  "up",
  "left",
  "ok",
  "down",
  "back",
  "home",
  "menu",
] as const;
const RIGHT_IDS = [
  "mic",
  "right",
  "volume_up",
  "volume_down",
  "tv",
] as const;

const DEFAULT_LABELS: Record<string, string> = {
  power: "电源",
  mic: "语音",
  up: "上",
  left: "左",
  ok: "确定",
  right: "右",
  down: "下",
  back: "返回",
  volume_up: "音量+",
  home: "主页",
  volume_down: "音量-",
  menu: "菜单",
  tv: "TV",
};

const selectedId = ref<string | null>(null);
const hoverId = ref<string | null>(null);
const capturing = ref(false);
const captureError = ref<string | null>(null);
const liveLabels = ref<string[]>([]);

const stageRef = ref<HTMLElement | null>(null);
const remoteRef = ref<InstanceType<typeof RemoteHotspot> | null>(null);
const cardRefs = ref<Record<string, HTMLElement | null>>({});

const linePath = ref("");
const lineOpacity = ref(0);
const lineStrong = ref(true);
const dotA = ref({ x: 0, y: 0 });
const dotB = ref({ x: 0, y: 0 });
const svgSize = ref({ w: 0, h: 0 });

let unlistenCaptured: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let applied = false;
let resizeObs: ResizeObserver | null = null;

function setCardRef(id: string, el: unknown) {
  cardRefs.value[id] = (el as HTMLElement) || null;
}

function labelOf(id: string): string {
  return props.config.button_aliases?.[id] || DEFAULT_LABELS[id] || id;
}

function actionOf(id: string): KeyAction {
  return (
    props.config.button_bindings?.[id] || { type: "None", value: null }
  );
}

function actionLabel(action: KeyAction): string {
  if (!action || action.type === "None") return "未绑定";
  if (action.type === "SingleKey") return vkDisplayName(Number(action.value));
  if (action.type === "ComboKey") {
    const arr = Array.isArray(action.value) ? action.value : [];
    return arr.map((v) => vkDisplayName(Number(v))).join(" + ");
  }
  if (action.type === "TextInput") return `文字: ${action.value}`;
  if (action.type === "LaunchApp") return `启动: ${action.value}`;
  return "—";
}

function pickMediaKey(vk: number) {
  if (!capturing.value) return;
  onCaptured([vk], [vkDisplayName(vk)]);
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

const leftButtons = computed(() =>
  LEFT_IDS.map((id) => ({
    id,
    label: labelOf(id),
    action: actionOf(id),
    side: "left" as const,
  }))
);

const rightButtons = computed(() =>
  RIGHT_IDS.map((id) => ({
    id,
    label: labelOf(id),
    action: actionOf(id),
    side: "right" as const,
  }))
);

const activeLineId = computed(
  () => selectedId.value || hoverId.value || null
);

function edgeToward(
  el: HTMLElement,
  stageBox: DOMRect,
  side: "left" | "right"
) {
  const r = el.getBoundingClientRect();
  const y = r.top + r.height / 2 - stageBox.top;
  // left 侧卡片：取右边缘；right 侧卡片：取左边缘
  if (side === "left") {
    return { x: r.right - stageBox.left, y };
  }
  return { x: r.left - stageBox.left, y };
}

/** 按键锚点：朝映射块一侧的边缘中点，避免线穿过键帽文字 */
function keyEdgeToward(
  el: HTMLElement,
  stageBox: DOMRect,
  side: "left" | "right"
) {
  const r = el.getBoundingClientRect();
  const y = r.top + r.height / 2 - stageBox.top;
  // 左栏连线接到按键左缘；右栏接到按键右缘
  if (side === "left") {
    return { x: r.left - stageBox.left, y };
  }
  return { x: r.right - stageBox.left, y };
}

function updateLine() {
  const id = activeLineId.value;
  const stage = stageRef.value;
  if (!id || !stage) {
    lineOpacity.value = 0;
    linePath.value = "";
    return;
  }

  const stageBox = stage.getBoundingClientRect();
  svgSize.value = { w: stageBox.width, h: stageBox.height };

  const card = cardRefs.value[id];
  const key = remoteRef.value?.keyEl?.(id) as HTMLElement | null;
  if (!card || !key) {
    lineOpacity.value = 0;
    linePath.value = "";
    return;
  }

  const side = (LEFT_IDS as readonly string[]).includes(id) ? "left" : "right";
  const keyPt = keyEdgeToward(key, stageBox, side);
  const cardPt = edgeToward(card, stageBox, side);

  const dx = Math.max(40, Math.abs(keyPt.x - cardPt.x) * 0.45);
  const c1 =
    side === "left"
      ? { x: cardPt.x + dx, y: cardPt.y }
      : { x: cardPt.x - dx, y: cardPt.y };
  const c2 =
    side === "left"
      ? { x: keyPt.x - dx * 0.25, y: keyPt.y }
      : { x: keyPt.x + dx * 0.25, y: keyPt.y };

  linePath.value = `M ${cardPt.x} ${cardPt.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${keyPt.x} ${keyPt.y}`;
  dotA.value = cardPt;
  dotB.value = keyPt;
  lineStrong.value = selectedId.value === id;
  lineOpacity.value = lineStrong.value ? 1 : 0.45;
}

async function selectButton(id: string) {
  selectedId.value = id;
  await nextTick();
  updateLine();
}

function onRemoteHover(id: string | null) {
  hoverId.value = id;
  updateLine();
}

function onCardHover(id: string | null) {
  hoverId.value = id;
  updateLine();
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

/** 录入期间拦截 WebView 加速键（Ctrl+A / Alt 菜单等）；OS 层吞键由 LL 钩子负责 */
function blockBrowserKeysDuringCapture(e: KeyboardEvent) {
  if (!capturing.value) return;
  e.preventDefault();
  e.stopPropagation();
}

function startPolling() {
  stopPolling();
  applied = false;
  pollTimer = setInterval(async () => {
    if (!capturing.value || applied) return;
    try {
      // 进度也走 IPC：部分机器上 emit("shortcut-capture-progress") 会丢/延迟，
      // 仅靠 listen 会出现「按着没反应，松手后其实已录入」。
      const snap = await invoke<{
        pending: { keys: number[]; labels: string[] } | null;
        progress: string[];
      }>("capture_shortcut_poll");
      if (Array.isArray(snap?.progress) && snap.progress.length > 0) {
        liveLabels.value = snap.progress;
      }
      const result = snap?.pending;
      if (result && Array.isArray(result.keys) && result.keys.length > 0) {
        onCaptured(result.keys, result.labels || []);
      }
    } catch (e) {
      console.warn("capture poll failed", e);
    }
  }, 50);
}

async function onCaptured(keys: number[], labels: string[]) {
  if (applied) return;
  applied = true;
  stopPolling();
  liveLabels.value = [];

  const buttonId = selectedId.value;
  if (buttonId && keys?.length) {
    applyCapturedKeys(buttonId, keys);
  }
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
  capturing.value = false;
  void nextTick().then(updateLine);
}

async function startCapture() {
  const buttonId = selectedId.value;
  if (!buttonId) return;
  if (capturing.value) {
    await cancelCapture();
    return;
  }
  captureError.value = null;
  capturing.value = true;
  liveLabels.value = [];
  applied = false;
  try {
    await invoke("capture_shortcut_start");
    startPolling();
  } catch (e) {
    capturing.value = false;
    stopPolling();
    captureError.value = String(e);
  }
}

async function cancelCapture() {
  stopPolling();
  capturing.value = false;
  liveLabels.value = [];
  applied = false;
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
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
}

watch([selectedId, hoverId], () => {
  void nextTick().then(updateLine);
});

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
      }
    );
  } catch (e) {
    console.warn("shortcut listen failed", e);
  }

  if (stageRef.value) {
    resizeObs = new ResizeObserver(() => updateLine());
    resizeObs.observe(stageRef.value);
  }
  stageRef.value?.addEventListener("scroll", updateLine, { passive: true });
  window.addEventListener("resize", updateLine);
  window.addEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.addEventListener("keyup", blockBrowserKeysDuringCapture, true);
});

onUnmounted(() => {
  stopPolling();
  unlistenCaptured?.();
  unlistenProgress?.();
  resizeObs?.disconnect();
  stageRef.value?.removeEventListener("scroll", updateLine);
  window.removeEventListener("resize", updateLine);
  window.removeEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.removeEventListener("keyup", blockBrowserKeysDuringCapture, true);
  if (capturing.value) {
    invoke("capture_shortcut_stop").catch(() => {});
  }
});
</script>

<template>
  <div class="stage-scroll">
    <div ref="stageRef" class="mapping-stage">
      <svg
        class="line-layer"
        :width="svgSize.w"
        :height="svgSize.h"
        :viewBox="`0 0 ${svgSize.w} ${svgSize.h}`"
        aria-hidden="true"
      >
        <path
          v-if="linePath"
          :d="linePath"
          fill="none"
          :stroke="lineStrong ? '#2563eb' : '#94a3b8'"
          :stroke-width="lineStrong ? 2.2 : 1.5"
          stroke-linecap="round"
          :opacity="lineOpacity"
        />
        <circle
          v-if="linePath"
          :cx="dotA.x"
          :cy="dotA.y"
          r="3.5"
          :fill="lineStrong ? '#2563eb' : '#94a3b8'"
          :opacity="lineOpacity"
        />
        <circle
          v-if="linePath"
          :cx="dotB.x"
          :cy="dotB.y"
          r="3.5"
          :fill="lineStrong ? '#2563eb' : '#94a3b8'"
          :opacity="lineOpacity"
        />
      </svg>

      <aside class="side-col left-col">
        <div
          v-for="btn in leftButtons"
          :key="btn.id"
          :ref="(el) => setCardRef(btn.id, el)"
          class="map-card"
          :class="{
            active: selectedId === btn.id,
            hover: hoverId === btn.id && selectedId !== btn.id,
          }"
          @mouseenter="onCardHover(btn.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(btn.id)"
        >
          <div class="map-card-main">
            <span class="map-name">{{ btn.label }}</span>
            <span
              :class="['map-bind', { unbound: btn.action.type === 'None' }]"
            >
              {{ actionLabel(btn.action) }}
            </span>
          </div>
          <div v-if="selectedId === btn.id" class="map-card-actions" @click.stop>
            <button
              type="button"
              class="btn-sm btn-edit"
              :disabled="capturing && selectedId !== btn.id"
              @click="startCapture"
            >
              {{ capturing && selectedId === btn.id ? "取消录入" : "录入" }}
            </button>
            <button
              v-if="btn.action.type !== 'None'"
              type="button"
              class="btn-sm btn-clear"
              :disabled="capturing"
              @click="clearBinding(btn.id)"
            >
              清除
            </button>
            <p
              v-if="capturing && selectedId === btn.id"
              class="capture-live"
              :class="{ 'capture-hint-blink': !liveLabels.length }"
            >
              {{
                liveLabels.length
                  ? liveLabels.join(" + ") + " …"
                  : "请按目标键或组合键"
              }}
            </p>
            <div
              v-if="capturing && selectedId === btn.id"
              class="media-pick"
            >
              <span class="media-pick-label">设置为：</span>
              <button
                v-for="k in MEDIA_PICK_KEYS"
                :key="k.vk"
                type="button"
                class="btn-sm btn-media"
                @click="pickMediaKey(k.vk)"
              >
                {{ k.label }}
              </button>
            </div>
            <p v-if="captureError && selectedId === btn.id" class="capture-err">
              {{ captureError }}
            </p>
          </div>
        </div>
      </aside>

      <div class="center-stage">
        <RemoteHotspot
          ref="remoteRef"
          :selected-id="selectedId"
          :hover-id="hoverId"
          @select="selectButton"
          @hover="onRemoteHover"
        />
      </div>

      <aside class="side-col right-col">
        <div
          v-for="btn in rightButtons"
          :key="btn.id"
          :ref="(el) => setCardRef(btn.id, el)"
          class="map-card"
          :class="{
            active: selectedId === btn.id,
            hover: hoverId === btn.id && selectedId !== btn.id,
          }"
          @mouseenter="onCardHover(btn.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(btn.id)"
        >
          <div class="map-card-main">
            <span class="map-name">{{ btn.label }}</span>
            <span
              :class="['map-bind', { unbound: btn.action.type === 'None' }]"
            >
              {{ actionLabel(btn.action) }}
            </span>
          </div>
          <div v-if="selectedId === btn.id" class="map-card-actions" @click.stop>
            <button
              type="button"
              class="btn-sm btn-edit"
              :disabled="capturing && selectedId !== btn.id"
              @click="startCapture"
            >
              {{ capturing && selectedId === btn.id ? "取消录入" : "录入" }}
            </button>
            <button
              v-if="btn.action.type !== 'None'"
              type="button"
              class="btn-sm btn-clear"
              :disabled="capturing"
              @click="clearBinding(btn.id)"
            >
              清除
            </button>
            <p
              v-if="capturing && selectedId === btn.id"
              class="capture-live"
              :class="{ 'capture-hint-blink': !liveLabels.length }"
            >
              {{
                liveLabels.length
                  ? liveLabels.join(" + ") + " …"
                  : "请按目标键或组合键"
              }}
            </p>
            <div
              v-if="capturing && selectedId === btn.id"
              class="media-pick"
            >
              <span class="media-pick-label">设置为：</span>
              <button
                v-for="k in MEDIA_PICK_KEYS"
                :key="k.vk"
                type="button"
                class="btn-sm btn-media"
                @click="pickMediaKey(k.vk)"
              >
                {{ k.label }}
              </button>
            </div>
            <p v-if="captureError && selectedId === btn.id" class="capture-err">
              {{ captureError }}
            </p>
          </div>
        </div>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.stage-scroll {
  overflow-x: auto;
  margin: 0 -4px;
  padding-bottom: 4px;
}

.mapping-stage {
  position: relative;
  display: grid;
  /* 左右平分剩余宽度；单侧最小宽度 100px（原 200 的一半） */
  grid-template-columns: minmax(100px, 1fr) auto minmax(100px, 1fr);
  gap: 10px 12px;
  align-items: start;
  min-width: 560px;
  width: 100%;
  padding: 4px 0 8px;
  box-sizing: border-box;
}

.line-layer {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 5;
  overflow: visible;
}

.side-col {
  display: flex;
  flex-direction: column;
  gap: 6px;
  z-index: 2;
  min-width: 0;
  width: 100%;
  padding-top: 0;
}

.left-col {
  align-items: stretch;
}

.right-col {
  align-items: stretch;
}

.center-stage {
  z-index: 2;
  justify-self: center;
  align-self: start;
  padding: 0;
  margin: 0;
  background: transparent;
  border: none;
  box-shadow: none;
}

.map-card {
  background: #fff;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  padding: 8px 10px;
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
  min-width: 0;
}

.map-card:hover,
.map-card.hover {
  border-color: #93c5fd;
  background: #f8fbff;
}

.map-card.active {
  border-color: #2563eb;
  box-shadow: 0 0 0 2px rgba(37, 99, 235, 0.15);
  background: #eff6ff;
}

.map-card-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.map-name {
  font-size: 13px;
  font-weight: 600;
  color: #0f172a;
  flex-shrink: 0;
}

.map-bind {
  font-size: 12px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  background: #f1f5f9;
  color: #334155;
  padding: 2px 8px;
  border-radius: 4px;
  min-width: 0;
  max-width: none;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: right;
}

.map-bind.unbound {
  background: transparent;
  color: #94a3b8;
}

.map-card-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: 10px;
  padding-top: 8px;
  border-top: 1px solid #e2e8f0;
}

.btn-sm {
  padding: 4px 10px;
  border: 1px solid #cbd5e1;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  background: #fff;
  color: #334155;
}

.btn-edit {
  color: #2563eb;
  border-color: #2563eb;
}
.btn-edit:hover:not(:disabled) {
  background: #eff6ff;
}
.btn-clear {
  color: #dc2626;
  border-color: #fecaca;
}
.btn-clear:hover:not(:disabled) {
  background: #fef2f2;
}
.btn-sm:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.capture-live {
  width: 100%;
  margin: 4px 0 0;
  font-size: 12px;
  color: #2563eb;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}

.capture-live.capture-hint-blink {
  text-align: center;
  color: #ea580c;
  font-weight: 600;
  animation: capture-hint-blink 1s ease-in-out infinite;
}

@keyframes capture-hint-blink {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.3;
  }
}

.media-pick {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  width: 100%;
  margin-top: 4px;
}

.media-pick-label {
  font-size: 12px;
  color: #64748b;
  flex-shrink: 0;
}

.btn-media {
  color: #0f766e;
  border-color: #99f6e4;
  background: #f0fdfa;
  padding: 3px 8px;
}
.btn-media:hover:not(:disabled) {
  background: #ccfbf1;
}

.capture-err {
  width: 100%;
  margin: 2px 0 0;
  font-size: 12px;
  color: #dc2626;
}
</style>
