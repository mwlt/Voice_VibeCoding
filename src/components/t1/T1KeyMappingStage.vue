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
import type { DeviceConfig, KeyAction } from "../../types";
import T1RemoteHotspot from "./T1RemoteHotspot.vue";
import T1KeyIcon from "./T1KeyIcon.vue";
import { MEDIA_PICK_KEYS, vkDisplayName } from "../../utils/vkDisplay";
import {
  T1_LEFT_COLUMN_IDS,
  T1_RIGHT_COLUMN_IDS,
  T1_VOICE_QUICK_PRESETS,
  T1_FIXED_SYSTEM_HINTS,
  applyT1CapturedBinding,
  applyT1VoiceQuick,
  clearT1Binding,
  t1ActionLabel,
  t1IsFixedSystemKey,
  t1LabelOf,
  type T1FixedSystemId,
  type T1VoiceQuickPreset,
} from "../../utils/t1Keys";

const props = defineProps<{
  config: DeviceConfig;
  hardwareLit?: boolean;
}>();

const emit = defineEmits<{
  save: [config: DeviceConfig];
}>();

const selectedId = ref<string | null>(null);
const hoverId = ref<string | null>(null);
const capturing = ref(false);
const captureError = ref<string | null>(null);
const liveLabels = ref<string[]>([]);

const stageRef = ref<HTMLElement | null>(null);
const remoteRef = ref<InstanceType<typeof T1RemoteHotspot> | null>(null);
const cardRefs = ref<Record<string, HTMLElement | null>>({});

const linePath = ref("");
const lineOpacity = ref(0);
const lineStrong = ref(true);
const lineFixed = ref(false);
const dotA = ref({ x: 0, y: 0 });
const dotB = ref({ x: 0, y: 0 });
const svgSize = ref({ w: 0, h: 0 });

let unlistenCaptured: UnlistenFn | null = null;
let unlistenProgress: UnlistenFn | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let applied = false;
let resizeObs: ResizeObserver | null = null;
let lineRaf: number | null = null;

/** 前端兜底和弦：LL 钩子被 T1 Raw Input 拖垮时仍可录入 */
const localHeld = new Set<number>();
let localChord: number[] = [];

function eventToVk(e: KeyboardEvent): number | null {
  // keyCode 与 Windows VK 对齐（录入目标即 VK）
  const vk = e.keyCode || e.which;
  if (!vk || vk === 0) return null;
  // 忽略纯修饰键抬起前的重复；修饰键本身可进入和弦
  return vk;
}

function resetLocalChord() {
  localHeld.clear();
  localChord = [];
}

/** 录入期间拦截 WebView 加速键，并做前端和弦兜底 */
function blockBrowserKeysDuringCapture(e: KeyboardEvent) {
  if (!capturing.value) return;
  e.preventDefault();
  e.stopPropagation();

  const vk = eventToVk(e);
  if (vk == null || applied) return;

  if (e.type === "keydown") {
    if (e.repeat) return;
    localHeld.add(vk);
    if (!localChord.includes(vk)) localChord.push(vk);
    liveLabels.value = localChord.map((k) => vkDisplayName(k));
    return;
  }

  if (e.type === "keyup") {
    localHeld.delete(vk);
    if (localHeld.size === 0 && localChord.length > 0) {
      const keys = [...localChord];
      const labels = keys.map((k) => vkDisplayName(k));
      resetLocalChord();
      onCaptured(keys, labels);
    }
  }
}

function setCardRef(id: string, el: unknown) {
  cardRefs.value[id] = (el as HTMLElement) || null;
}

function actionOf(id: string): KeyAction {
  return (
    props.config.button_bindings?.[id] || { type: "None", value: null }
  );
}

function pickMediaKey(vk: number) {
  if (!capturing.value) return;
  onCaptured([vk], [vkDisplayName(vk)]);
}

const voiceQuickPresets = T1_VOICE_QUICK_PRESETS;
const voiceQuickPressedId = ref<string | null>(null);

function applyVoiceQuick(item: T1VoiceQuickPreset, e: MouseEvent) {
  voiceQuickPressedId.value = item.id;
  window.setTimeout(() => {
    if (voiceQuickPressedId.value === item.id) voiceQuickPressedId.value = null;
  }, 160);
  emit("save", applyT1VoiceQuick(props.config, item));
  (e.currentTarget as HTMLButtonElement).blur();
  if (selectedId.value === "voice" || hoverId.value === "voice") {
    void nextTick().then(scheduleUpdateLine);
  }
}

const leftButtons = computed(() =>
  T1_LEFT_COLUMN_IDS.map((id) => ({
    id,
    label: t1LabelOf(id, props.config.button_aliases),
    action: actionOf(id),
    side: "left" as const,
  }))
);

const rightButtons = computed(() =>
  T1_RIGHT_COLUMN_IDS.map((id) => ({
    id,
    label: t1LabelOf(id, props.config.button_aliases),
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
  if (side === "left") {
    return { x: r.right - stageBox.left, y };
  }
  return { x: r.left - stageBox.left, y };
}

function keyEdgeToward(
  el: HTMLElement,
  stageBox: DOMRect,
  side: "left" | "right"
) {
  const r = el.getBoundingClientRect();
  const y = r.top + r.height / 2 - stageBox.top;
  if (side === "left") {
    return { x: r.left - stageBox.left, y };
  }
  return { x: r.right - stageBox.left, y };
}

function scheduleUpdateLine() {
  if (lineRaf != null) return;
  lineRaf = requestAnimationFrame(() => {
    lineRaf = null;
    updateLine();
  });
}

function updateLine() {
  const id = activeLineId.value;
  const stage = stageRef.value;
  if (!id || !stage) {
    if (lineOpacity.value !== 0) lineOpacity.value = 0;
    if (linePath.value) linePath.value = "";
    return;
  }

  const stageBox = stage.getBoundingClientRect();
  const w = Math.round(stageBox.width);
  const h = Math.round(stageBox.height);
  if (svgSize.value.w !== w || svgSize.value.h !== h) {
    svgSize.value = { w, h };
  }

  const card = cardRefs.value[id];
  const key = remoteRef.value?.keyEl?.(id) as HTMLElement | null;
    if (!card || !key) {
    if (lineOpacity.value !== 0) lineOpacity.value = 0;
    if (linePath.value) linePath.value = "";
    return;
  }

  const side = (T1_LEFT_COLUMN_IDS as readonly string[]).includes(id)
    ? "left"
    : "right";
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

  const nextPath = `M ${cardPt.x} ${cardPt.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${keyPt.x} ${keyPt.y}`;
  const strong = selectedId.value === id;
  const fixed = t1IsFixedSystemKey(id);
  const opacity = fixed ? 0.55 : strong ? 1 : 0.45;
  if (linePath.value !== nextPath) linePath.value = nextPath;
  if (dotA.value.x !== cardPt.x || dotA.value.y !== cardPt.y) dotA.value = cardPt;
  if (dotB.value.x !== keyPt.x || dotB.value.y !== keyPt.y) dotB.value = keyPt;
  if (lineStrong.value !== strong) lineStrong.value = strong;
  if (lineFixed.value !== fixed) lineFixed.value = fixed;
  if (lineOpacity.value !== opacity) lineOpacity.value = opacity;
}

function fixedHintOf(id: string): string {
  if (!t1IsFixedSystemKey(id)) return "";
  return T1_FIXED_SYSTEM_HINTS[id as T1FixedSystemId];
}

async function selectButton(id: string) {
  if (t1IsFixedSystemKey(id)) {
    // 电源 / 鼠标：仅展示说明，不进入选中与录入
    return;
  }
  if (selectedId.value === id) {
    if (capturing.value) {
      await cancelCapture();
    }
    selectedId.value = null;
    captureError.value = null;
    await nextTick();
    updateLine();
    return;
  }
  if (capturing.value) {
    await cancelCapture();
  }
  selectedId.value = id;
  captureError.value = null;
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

function startPolling() {
  stopPolling();
  applied = false;
  pollTimer = setInterval(async () => {
    if (!capturing.value || applied) return;
    try {
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
      console.warn("t1 capture poll failed", e);
    }
  }, 50);
}

async function onCaptured(keys: number[], _labels: string[]) {
  if (applied) return;
  applied = true;
  stopPolling();
  resetLocalChord();
  liveLabels.value = [];

  const buttonId = selectedId.value;
  if (buttonId && keys?.length) {
    emit("save", applyT1CapturedBinding(props.config, buttonId, keys));
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
  if (!buttonId || t1IsFixedSystemKey(buttonId)) return;
  if (capturing.value) {
    await cancelCapture();
    return;
  }
  captureError.value = null;
  capturing.value = true;
  liveLabels.value = [];
  applied = false;
  resetLocalChord();
  // 避免录入按钮吃掉 Space/Enter
  (document.activeElement as HTMLElement | null)?.blur?.();
  try {
    await invoke("capture_shortcut_start");
    startPolling();
  } catch (e) {
    // 钩子失败时仍可用前端兜底和弦录入
    console.warn("t1 capture_shortcut_start failed, using local chord fallback", e);
    captureError.value = null;
    startPolling();
  }
}

async function cancelCapture() {
  stopPolling();
  capturing.value = false;
  liveLabels.value = [];
  applied = false;
  resetLocalChord();
  try {
    await invoke("capture_shortcut_stop");
  } catch {
    /* ignore */
  }
}

function onClearBinding(buttonId: string) {
  emit("save", clearT1Binding(props.config, buttonId));
}

watch([selectedId, hoverId], () => {
  void nextTick().then(scheduleUpdateLine);
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
    console.warn("t1 shortcut listen failed", e);
  }

  if (stageRef.value) {
    resizeObs = new ResizeObserver(() => scheduleUpdateLine());
    resizeObs.observe(stageRef.value);
  }
  stageRef.value?.addEventListener("scroll", scheduleUpdateLine, {
    passive: true,
  });
  window.addEventListener("resize", scheduleUpdateLine);
  window.addEventListener("keydown", blockBrowserKeysDuringCapture, true);
  window.addEventListener("keyup", blockBrowserKeysDuringCapture, true);
});

onUnmounted(() => {
  stopPolling();
  unlistenCaptured?.();
  unlistenProgress?.();
  resizeObs?.disconnect();
  if (lineRaf != null) {
    cancelAnimationFrame(lineRaf);
    lineRaf = null;
  }
  stageRef.value?.removeEventListener("scroll", scheduleUpdateLine);
  window.removeEventListener("resize", scheduleUpdateLine);
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
        :viewBox="`0 0 ${svgSize.w || 1} ${svgSize.h || 1}`"
        aria-hidden="true"
      >
        <path
          v-if="linePath"
          :d="linePath"
          fill="none"
          :stroke="
            lineFixed ? '#6b8499' : lineStrong ? '#2563eb' : '#94a3b8'
          "
          :stroke-width="lineStrong && !lineFixed ? 2.2 : 1.5"
          stroke-linecap="round"
          :opacity="lineOpacity"
        />
        <circle
          v-if="linePath"
          :cx="dotA.x"
          :cy="dotA.y"
          r="3.5"
          :fill="lineFixed ? '#6b8499' : lineStrong ? '#2563eb' : '#94a3b8'"
          :opacity="lineOpacity"
        />
        <circle
          v-if="linePath"
          :cx="dotB.x"
          :cy="dotB.y"
          r="3.5"
          :fill="lineFixed ? '#6b8499' : lineStrong ? '#2563eb' : '#94a3b8'"
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
            active: !t1IsFixedSystemKey(btn.id) && selectedId === btn.id,
            hover:
              !t1IsFixedSystemKey(btn.id) &&
              hoverId === btn.id &&
              selectedId !== btn.id,
            'map-card-fixed': t1IsFixedSystemKey(btn.id),
            'map-card-fixed-hover':
              t1IsFixedSystemKey(btn.id) && hoverId === btn.id,
          }"
          @mouseenter="onCardHover(btn.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(btn.id)"
        >
          <div class="map-card-main">
            <span
              class="map-name"
              :class="{ 'map-name-fixed': t1IsFixedSystemKey(btn.id) }"
            >
              <T1KeyIcon :key-id="btn.id" />
              {{ btn.label }}
            </span>
            <span
              v-if="t1IsFixedSystemKey(btn.id)"
              class="map-bind map-bind-fixed"
              :title="fixedHintOf(btn.id)"
            >
              {{ fixedHintOf(btn.id) }}
            </span>
            <span
              v-else
              :class="['map-bind', { unbound: btn.action.type === 'None' }]"
            >
              {{ t1ActionLabel(btn.action) }}
            </span>
          </div>
          <div
            v-if="!t1IsFixedSystemKey(btn.id) && selectedId === btn.id"
            class="map-card-actions"
            @click.stop
          >
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
              @click="onClearBinding(btn.id)"
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

        <div class="voice-note" aria-label="连接方式说明">
          <section class="voice-note-block">
            <h4>连接方式差异</h4>
            <p>
              <b>USB连接：</b>T1 语音 HID 即使物理长按也只有约 120ms 脉冲，语音使用点击说话，点击结束方式。USB 麦克约 15–16s 后会硬件静音，建议每次说话 16 秒内。输入法麦克风选择 Mic Device。
            </p>
            <p>
              <b>蓝牙连接：</b>同样是点击说话，再点击结束方式。蓝牙连接走独立 ATVV 会话（MIC_EXTEND），PCM 进 VB-CABLE；说话时长无限制。输入法麦克风选择 CABLE Output。
            </p>
          </section>
        </div>
      </aside>

      <div class="center-stage">
        <T1RemoteHotspot
          ref="remoteRef"
          :selected-id="selectedId"
          :hover-id="hoverId"
          :hardware-lit="hardwareLit"
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
            active: !t1IsFixedSystemKey(btn.id) && selectedId === btn.id,
            hover:
              !t1IsFixedSystemKey(btn.id) &&
              hoverId === btn.id &&
              selectedId !== btn.id,
            'map-card-fixed': t1IsFixedSystemKey(btn.id),
            'map-card-fixed-hover':
              t1IsFixedSystemKey(btn.id) && hoverId === btn.id,
          }"
          @mouseenter="onCardHover(btn.id)"
          @mouseleave="onCardHover(null)"
          @click="selectButton(btn.id)"
        >
          <div class="map-card-main">
            <span
              class="map-name"
              :class="{ 'map-name-fixed': t1IsFixedSystemKey(btn.id) }"
            >
              <T1KeyIcon :key-id="btn.id" />
              {{ btn.label }}
            </span>
            <span
              v-if="t1IsFixedSystemKey(btn.id)"
              class="map-bind map-bind-fixed"
              :title="fixedHintOf(btn.id)"
            >
              {{ fixedHintOf(btn.id) }}
            </span>
            <span
              v-else
              :class="['map-bind', { unbound: btn.action.type === 'None' }]"
            >
              {{ t1ActionLabel(btn.action) }}
            </span>
          </div>
          <div
            v-if="!t1IsFixedSystemKey(btn.id) && selectedId === btn.id"
            class="map-card-actions"
            @click.stop
          >
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
              @click="onClearBinding(btn.id)"
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

        <div class="voice-quick-setup" aria-label="语音键快速设置">
          <p class="voice-quick-label">将语音键设置为：</p>
          <div class="voice-quick-grid">
            <button
              v-for="item in voiceQuickPresets"
              :key="item.id"
              type="button"
              class="voice-quick-btn"
              :class="{ pressed: voiceQuickPressedId === item.id }"
              :aria-label="`将语音键设置为 ${item.segments.join(' 加 ')}`"
              @click="applyVoiceQuick(item, $event)"
            >
              <span class="voice-quick-chord">
                <template v-for="(seg, segIdx) in item.segments" :key="seg">
                  <span v-if="segIdx > 0" class="chord-plus" aria-hidden="true">+</span>
                  <kbd class="key-cap-chip">{{ seg }}</kbd>
                </template>
              </span>
            </button>
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
  grid-template-columns: minmax(100px, 1fr) auto minmax(100px, 1fr);
  gap: 10px 12px;
  align-items: start;
  min-width: 560px;
  width: 100%;
  padding: 8px 0 6px;
  box-sizing: border-box;
}

.line-layer {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
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
}

.center-stage {
  z-index: 2;
  justify-self: center;
  align-self: start;
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

.map-card-fixed {
  cursor: default;
  border-color: #e6eaee;
  background: #f4f6f8;
}

.map-card-fixed:hover,
.map-card.map-card-fixed-hover {
  border-color: #dde3e9;
  background: #f1f4f7;
  box-shadow: none;
}

.map-name-fixed {
  color: #5f738a;
}

.map-name-fixed :deep(.t1-key-icon) {
  color: #6b8499;
  border-color: #9aafc2;
  background: rgba(107, 132, 153, 0.12);
}

.map-bind-fixed {
  background: transparent;
  color: #6b8499;
  font-family: inherit;
  font-size: 11px;
  font-weight: 500;
  white-space: nowrap;
}

.map-card-main {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.map-name {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 400;
  color: #0f172a;
  flex-shrink: 0;
  min-width: 0;
}

.map-bind {
  font-size: 12px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  background: #f1f5f9;
  color: #334155;
  padding: 2px 8px;
  border-radius: 4px;
  min-width: 0;
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

.voice-quick-setup {
  margin-top: 4px;
  padding: 10px 10px 11px;
  border-radius: 10px;
  border: 1px solid #e2e8f0;
  background: linear-gradient(180deg, #fafbfd 0%, #f8fafc 100%);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.85);
}

.voice-quick-label {
  margin: 0 0 8px;
  font-size: 12px;
  font-weight: 600;
  color: #475569;
  letter-spacing: 0.01em;
}

.voice-quick-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 7px;
}

.voice-note {
  margin-top: 4px;
  padding: 2px 2px 0;
  border: 1px solid transparent;
  background: transparent;
  box-shadow: none;
}
.voice-note-block h4 {
  margin: 0 0 6px;
  font-size: 12px;
  font-weight: 600;
  color: #475569;
  letter-spacing: 0.01em;
}
.voice-note-block p {
  margin: 0 0 6px;
  font-size: 11.5px;
  line-height: 1.55;
  color: #64748b;
}
.voice-note-block p:last-child {
  margin-bottom: 0;
}
.voice-note-block b {
  font-weight: 600;
  color: #475569;
}

.voice-quick-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 38px;
  padding: 6px 8px;
  border: 1px solid #c5d0de;
  border-radius: 9px;
  background: #f3f6fa;
  cursor: pointer;
  transition:
    background 0.14s ease,
    border-color 0.14s ease,
    box-shadow 0.14s ease,
    transform 0.1s ease;
  box-shadow: none;
}

.voice-quick-btn:hover {
  background: #dbeafe;
  border-color: #93c5fd;
}

.voice-quick-btn:focus {
  outline: none;
}

.voice-quick-btn:focus-visible {
  outline: 2px solid #60a5fa;
  outline-offset: 2px;
}

.voice-quick-btn:active,
.voice-quick-btn.pressed {
  transform: translateY(1px);
  background: #bfdbfe;
  border-color: #60a5fa;
  box-shadow: none;
}

.voice-quick-chord {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: 3px;
  max-width: 100%;
}

.chord-plus {
  font-size: 11px;
  font-weight: 600;
  color: #94a3b8;
  line-height: 1;
  user-select: none;
}

.key-cap-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 22px;
  padding: 2px 7px;
  border-radius: 6px;
  border: 1px solid #d5dee9;
  background: #f8fafc;
  color: #334155;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 10.5px;
  font-weight: 600;
  line-height: 1.2;
  white-space: nowrap;
  box-shadow: none;
  transition:
    transform 0.1s ease,
    box-shadow 0.1s ease,
    background 0.14s ease,
    border-color 0.14s ease;
}

.voice-quick-btn:hover .key-cap-chip {
  background: #eff6ff;
  border-color: #93c5fd;
}

.voice-quick-btn:active .key-cap-chip,
.voice-quick-btn.pressed .key-cap-chip {
  transform: translateY(1px);
  background: #dbeafe;
  border-color: #60a5fa;
  box-shadow: none;
}
</style>
