<script setup lang="ts">
/**
 * T1 示意遥控器：对齐实物正面 + 右视图
 * （瘦长机身、上浅下深、顶孔+指示灯、右侧音量拨杆、底部握持区）
 * 指示灯/麦孔：平时与麦同色；按下短绿闪、长按常绿（麦更暗）。
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const props = defineProps<{
  selectedId?: string | null;
  hoverId?: string | null;
  /** 页面收到实体键 USB/BLE 事件时点亮 */
  hardwareLit?: boolean;
}>();

const emit = defineEmits<{
  select: [buttonId: string];
  hover: [buttonId: string | null];
}>();

const rootRef = ref<HTMLElement | null>(null);
const pointerHeld = ref(false);
const hwLit = ref(false);
const lit = computed(
  () => pointerHeld.value || hwLit.value || !!props.hardwareLit
);

const SHORT_MIN_MS = 90;
let pointerDownAt = 0;
let releaseTimer: ReturnType<typeof setTimeout> | null = null;
let flashTimer: ReturnType<typeof setTimeout> | null = null;
let unlistenKey: UnlistenFn | null = null;
let unlistenBle: UnlistenFn | null = null;
const hwDownIds = new Set<string>();

function syncHwLit() {
  hwLit.value = hwDownIds.size > 0;
}

function applyHwPayload(p: {
  id?: string;
  pressed?: boolean;
  message?: string;
  phase?: string;
}) {
  const msg = p.message || "";
  const id = p.id || "";
  let pressed: boolean | undefined =
    typeof p.pressed === "boolean" ? p.pressed : undefined;
  if (pressed === undefined) {
    if (msg.includes("↓") || msg.includes("按下")) pressed = true;
    else if (msg.includes("↑") || msg.includes("抬起")) pressed = false;
  }
  if (pressed === true) {
    if (flashTimer) {
      clearTimeout(flashTimer);
      flashTimer = null;
    }
    hwDownIds.add(id || "anon");
    syncHwLit();
    return;
  }
  if (pressed === false) {
    hwDownIds.delete(id || "anon");
    if (!id) hwDownIds.clear();
    syncHwLit();
    return;
  }
  if (id || p.phase === "key") {
    if (flashTimer) clearTimeout(flashTimer);
    hwLit.value = true;
    flashTimer = setTimeout(() => {
      flashTimer = null;
      if (hwDownIds.size === 0) hwLit.value = false;
    }, 140);
  }
}

function keyEl(id: string): HTMLElement | null {
  return (
    rootRef.value?.querySelector(`[data-key-id="${id}"]`) as HTMLElement | null
  );
}

function startPointerLit() {
  if (releaseTimer) {
    clearTimeout(releaseTimer);
    releaseTimer = null;
  }
  pointerHeld.value = true;
  pointerDownAt = Date.now();
}

function endPointerLit() {
  if (!pointerHeld.value) return;
  const remain = SHORT_MIN_MS - (Date.now() - pointerDownAt);
  if (remain > 0) {
    releaseTimer = setTimeout(() => {
      pointerHeld.value = false;
      releaseTimer = null;
    }, remain);
  } else {
    pointerHeld.value = false;
  }
}

function onRemotePointerDown(e: PointerEvent) {
  if (e.pointerType === "mouse" && e.button !== 0) return;
  const key = (e.target as HTMLElement | null)?.closest?.("[data-key-id]");
  if (!key || !rootRef.value?.contains(key)) return;
  startPointerLit();
}

function onWindowPointerUp() {
  endPointerLit();
}

onMounted(async () => {
  window.addEventListener("pointerup", onWindowPointerUp);
  window.addEventListener("pointercancel", onWindowPointerUp);
  try {
    unlistenKey = await listen<{
      id?: string;
      pressed?: boolean;
      message?: string;
    }>("t1-key", (ev) => applyHwPayload(ev.payload || {}));
    unlistenBle = await listen<{
      id?: string;
      pressed?: boolean;
      message?: string;
      phase?: string;
    }>("t1-ble", (ev) => applyHwPayload(ev.payload || {}));
  } catch {
    /* 浏览器预览无 Tauri */
  }
});

onUnmounted(() => {
  window.removeEventListener("pointerup", onWindowPointerUp);
  window.removeEventListener("pointercancel", onWindowPointerUp);
  if (releaseTimer) clearTimeout(releaseTimer);
  if (flashTimer) clearTimeout(flashTimer);
  unlistenKey?.();
  unlistenBle?.();
});

defineExpose({ keyEl, rootRef });
</script>

<template>
  <div
    ref="rootRef"
    class="t1-remote"
    aria-label="T1 遥控器示意"
    @pointerdown="onRemotePointerDown"
  >
    <div class="bezel">
      <div class="face">
        <div class="panel panel-upper">
          <div class="head-row">
            <div class="top-marks" aria-hidden="true">
              <span class="mic-hole" :class="{ 'is-lit': lit }" />
              <span class="led" :class="{ 'is-lit': lit }" />
            </div>
            <button
              type="button"
              class="key-cap key-cap-power key-fixed"
              data-key-id="power"
              aria-label="电源（作用为系统电源键，不可绑定）"
              title="作用为系统电源键 · 不可绑定"
              :class="{
                hover: hoverId === 'power',
              }"
              @mouseenter="emit('hover', 'power')"
              @mouseleave="emit('hover', null)"
            >
              <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
                <path
                  d="M12 4v7"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.55"
                  stroke-linecap="round"
                />
                <path
                  d="M7.35 6.8a6.65 6.65 0 1 0 9.3 0"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.55"
                  stroke-linecap="round"
                />
              </svg>
            </button>
          </div>

          <div class="dpad-wrap">
            <div class="dpad">
              <button
                type="button"
                class="key-ok"
                data-key-id="ok"
                aria-label="确定"
                :class="{ active: selectedId === 'ok', hover: hoverId === 'ok' }"
                @mouseenter="emit('hover', 'ok')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'ok')"
              />
              <button
                type="button"
                class="dpad-dir dpad-up"
                data-key-id="up"
                aria-label="上"
                :class="{ active: selectedId === 'up', hover: hoverId === 'up' }"
                @mouseenter="emit('hover', 'up')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'up')"
              >
                <span class="dpad-dot" />
              </button>
              <button
                type="button"
                class="dpad-dir dpad-left"
                data-key-id="left"
                aria-label="左"
                :class="{
                  active: selectedId === 'left',
                  hover: hoverId === 'left',
                }"
                @mouseenter="emit('hover', 'left')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'left')"
              >
                <span class="dpad-dot" />
              </button>
              <button
                type="button"
                class="dpad-dir dpad-right"
                data-key-id="right"
                aria-label="右"
                :class="{
                  active: selectedId === 'right',
                  hover: hoverId === 'right',
                }"
                @mouseenter="emit('hover', 'right')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'right')"
              >
                <span class="dpad-dot" />
              </button>
              <button
                type="button"
                class="dpad-dir dpad-down"
                data-key-id="down"
                aria-label="下"
                :class="{
                  active: selectedId === 'down',
                  hover: hoverId === 'down',
                }"
                @mouseenter="emit('hover', 'down')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'down')"
              >
                <span class="dpad-dot" />
              </button>
            </div>
          </div>

          <div class="mid-row">
            <button
              type="button"
              class="key-cap key-cap-mid"
              data-key-id="delete"
              aria-label="删除"
              :class="{
                active: selectedId === 'delete',
                hover: hoverId === 'delete',
              }"
              @mouseenter="emit('hover', 'delete')"
              @mouseleave="emit('hover', null)"
              @click="emit('select', 'delete')"
            >
              <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
                <path
                  d="M11.1 6.2 4.4 12l6.7 5.8v-3.4H19.8V9.6H11.1V6.2z"
                  fill="currentColor"
                />
              </svg>
            </button>
            <button
              type="button"
              class="key-cap key-cap-mid"
              data-key-id="voice"
              aria-label="语音"
              :class="{
                active: selectedId === 'voice',
                hover: hoverId === 'voice',
              }"
              @mouseenter="emit('hover', 'voice')"
              @mouseleave="emit('hover', null)"
              @click="emit('select', 'voice')"
            >
              <svg
                class="key-icon key-icon-assistant"
                viewBox="0 0 24 24"
                aria-hidden="true"
              >
                <circle cx="8.2" cy="14.2" r="2.35" fill="currentColor" />
                <circle cx="13.4" cy="16.1" r="1.55" fill="currentColor" />
                <circle cx="15.9" cy="11.4" r="2.05" fill="currentColor" />
                <circle cx="11.1" cy="8.6" r="2.9" fill="currentColor" />
              </svg>
            </button>
          </div>
        </div>

        <div class="panel panel-lower">
          <div class="lower-grid">
            <div class="mute-mouse-pill" role="group" aria-label="静音与鼠标">
              <button
                type="button"
                class="pill-half"
                data-key-id="mute"
                aria-label="静音"
                :class="{
                  active: selectedId === 'mute',
                  hover: hoverId === 'mute',
                }"
                @mouseenter="emit('hover', 'mute')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'mute')"
              >
                <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M4.5 10.2v3.6h2.6L11 17.5V6.5L7.1 10.2H4.5z"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.4"
                    stroke-linejoin="round"
                  />
                  <path
                    d="M15.2 9.2 20 14M20 9.2l-4.8 4.8"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.45"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
              <button
                type="button"
                class="pill-half key-fixed"
                data-key-id="mouse"
                aria-label="鼠标（遥控器内部按键，不可绑定）"
                title="遥控器内部按键 · 不可绑定"
                :class="{
                  hover: hoverId === 'mouse',
                }"
                @mouseenter="emit('hover', 'mouse')"
                @mouseleave="emit('hover', null)"
              >
                <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M9.15 7.55h5.7a2.15 2.15 0 0 1 2.15 2.15v6.5a3.35 3.35 0 0 1-3.35 3.35h-3.3A3.35 3.35 0 0 1 7 16.2V9.7a2.15 2.15 0 0 1 2.15-2.15z"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.35"
                  />
                  <path
                    d="M8.9 11.15h6.2"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.3"
                  />
                  <path
                    d="M12 7.55v3.6"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.3"
                    stroke-linecap="round"
                  />
                  <path
                    d="M7.05 8.35c-.75.4-.55.95-1.3 1.3M6.25 10.35c-.8.42-.58 1-1.35 1.32M16.95 8.35c.75.4.55.95 1.3 1.3M17.75 10.35c.8.42.58 1 1.35 1.32"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.25"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
            </div>

            <div class="lower-right">
              <button
                type="button"
                class="key-cap key-cap-lower"
                data-key-id="home"
                aria-label="主页"
                :class="{
                  active: selectedId === 'home',
                  hover: hoverId === 'home',
                }"
                @mouseenter="emit('hover', 'home')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'home')"
              >
                <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M5.2 11.4 12 5.6l6.8 5.8"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  />
                  <path
                    d="M7.4 10.5V17.6h9.2V10.5"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linejoin="round"
                  />
                </svg>
              </button>
              <button
                type="button"
                class="key-cap key-cap-lower"
                data-key-id="menu"
                aria-label="菜单"
                :class="{
                  active: selectedId === 'menu',
                  hover: hoverId === 'menu',
                }"
                @mouseenter="emit('hover', 'menu')"
                @mouseleave="emit('hover', null)"
                @click="emit('select', 'menu')"
              >
                <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M7 8.2h10M7 12h10M7 15.8h10"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.55"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
            </div>
          </div>

          <div class="grip" aria-hidden="true" />
        </div>
      </div>
    </div>

    <div class="side-vol" role="group" aria-label="音量">
      <button
        type="button"
        class="vol-half"
        data-key-id="vol_plus"
        aria-label="音量+"
        :class="{
          active: selectedId === 'vol_plus',
          hover: hoverId === 'vol_plus',
        }"
        @mouseenter="emit('hover', 'vol_plus')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'vol_plus')"
      >
        <svg class="vol-mark" viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M12 7.5v9M7.5 12h9"
            fill="none"
            stroke="currentColor"
            stroke-width="2.1"
            stroke-linecap="round"
          />
        </svg>
      </button>
      <button
        type="button"
        class="vol-half"
        data-key-id="vol_minus"
        aria-label="音量-"
        :class="{
          active: selectedId === 'vol_minus',
          hover: hoverId === 'vol_minus',
        }"
        @mouseenter="emit('hover', 'vol_minus')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'vol_minus')"
      >
        <svg class="vol-mark" viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M7.5 12h9"
            fill="none"
            stroke="currentColor"
            stroke-width="2.1"
            stroke-linecap="round"
          />
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.t1-remote {
  --power: 22px;
  --dpad: 96px;
  --band: 10px;
  --bezel-pad: 4px;
  --pill-h: calc(var(--round-key) * 2 + var(--round-gap));
  --head-h: calc(var(--power) * 1.5);
  --mic: 2.5px;
  --led-h: 7.5px;
  --marks-h: calc(var(--mic) + var(--mic) + var(--led-h));
  --round-key: 42px;
  --face-w: calc((42px * 2 + 12.5px * 3) * 1.1);
  --side-gap: calc((var(--face-w) - var(--round-key) * 2) / 3);
  --lower-pad-x: var(--side-gap);
  --round-gap: var(--side-gap);
  --bezel-w: calc(var(--face-w) + var(--bezel-pad) * 2);
  --vol-w: 14px;
  --vol-h: calc(var(--dpad) * 2 / 3);
  --vol-gap: calc(var(--vol-w) / 2);
  --dpad-top: calc(
    var(--bezel-pad) + var(--pad-top) + var(--head-h) + var(--band)
  );
  --pad-top: calc(
    var(--band) + (var(--head-h) - var(--power)) - var(--bezel-pad) -
      (var(--power) - var(--marks-h))
  );
  position: relative;
  width: calc(var(--bezel-w) + var(--vol-gap) + var(--vol-w));
  padding-right: calc(var(--vol-gap) + var(--vol-w));
  box-sizing: border-box;
  user-select: none;
  /* 与小米方向圆环 116px 对齐：96 × 116/96 */
  zoom: calc(116 / 96);
}

.bezel {
  width: var(--bezel-w);
  border-radius: 20px;
  padding: var(--bezel-pad);
  box-sizing: border-box;
  background: linear-gradient(
    155deg,
    #d8dde4 0%,
    #b7bec8 38%,
    #8f98a4 70%,
    #c5cbd3 100%
  );
  box-shadow:
    0 1px 0 rgba(255, 255, 255, 0.55) inset,
    0 -1px 0 rgba(15, 23, 42, 0.16) inset;
}

.face {
  overflow: hidden;
  border-radius: 16px;
}

.panel {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.panel-upper {
  gap: var(--band);
  padding: var(--pad-top) var(--lower-pad-x) 12px;
  background: linear-gradient(180deg, #4a4f56 0%, #3a3f46 100%);
}

.panel-lower {
  padding: 12px var(--lower-pad-x) 0;
  background: linear-gradient(180deg, #1b1d21 0%, #101214 100%);
}

.head-row {
  position: relative;
  width: 100%;
  height: var(--head-h);
}

.top-marks {
  position: absolute;
  left: 50%;
  top: calc(var(--power) - var(--marks-h));
  transform: translateX(-50%);
  height: var(--marks-h);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--mic);
}

.mic-hole {
  width: var(--mic);
  height: var(--mic);
  flex-shrink: 0;
  border-radius: 50%;
  background: #15171a;
  box-shadow:
    inset 0 0.5px 0.5px rgba(0, 0, 0, 0.65),
    0 0 0 0.5px rgba(255, 255, 255, 0.08);
  transition: background 0.08s ease, box-shadow 0.08s ease;
}
.mic-hole.is-lit {
  background: #166534;
  box-shadow:
    0 0 2px rgba(22, 163, 74, 0.4),
    0 0 0 0.5px rgba(34, 197, 94, 0.25);
}

.led {
  width: 1.5px;
  height: var(--led-h);
  flex-shrink: 0;
  border-radius: 0.5px;
  background: #15171a;
  box-shadow:
    inset 0 0.5px 0.5px rgba(0, 0, 0, 0.65),
    0 0 0 0.5px rgba(255, 255, 255, 0.08);
  transition: background 0.08s ease, box-shadow 0.08s ease;
}
.led.is-lit {
  background: #22c55e;
  box-shadow:
    0 0 5px rgba(34, 197, 94, 0.95),
    0 0 9px rgba(34, 197, 94, 0.5);
}

.key-icon {
  width: 14px;
  height: 14px;
  display: block;
  pointer-events: none;
}

.key-icon-assistant {
  width: 15px;
  height: 15px;
}

.key-cap {
  border: none;
  border-radius: 50%;
  padding: 0;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: #dfe3e8;
  background: #26292f;
  transition:
    background 0.15s,
    box-shadow 0.15s,
    color 0.15s;
}

.key-cap-power {
  position: absolute;
  top: calc(var(--power) / 2);
  right: 1px;
  width: var(--power);
  height: var(--power);
}

.key-cap-mid,
.key-cap-lower {
  width: var(--round-key);
  height: var(--round-key);
}

.key-cap:hover,
.key-cap.hover,
.key-cap.active,
.dpad-dir:hover,
.dpad-dir.hover,
.dpad-dir.active,
.key-ok:hover,
.key-ok.hover,
.key-ok.active,
.pill-half:hover,
.pill-half.hover,
.pill-half.active,
.vol-half:hover,
.vol-half.hover,
.vol-half.active {
  color: #eaf2ff;
  background: rgba(37, 99, 235, 0.92);
  box-shadow: inset 0 0 0 1.5px rgba(147, 197, 253, 0.95);
}

/* 电源 / 鼠标：灰蓝固定键，不走可映射高亮 */
.key-cap.key-fixed,
.pill-half.key-fixed {
  color: #8fa3b8;
  background: #2a3038;
  cursor: default;
  box-shadow: inset 0 0 0 1px rgba(143, 163, 184, 0.35);
}

.key-cap.key-fixed:hover,
.key-cap.key-fixed.hover,
.pill-half.key-fixed:hover,
.pill-half.key-fixed.hover {
  color: #a8bbcf;
  background: #323a44;
  box-shadow: inset 0 0 0 1.5px rgba(143, 163, 184, 0.55);
}

.dpad-wrap {
  width: var(--dpad);
  height: var(--dpad);
}

.dpad {
  position: relative;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  background: #16181c;
  overflow: hidden;
  box-shadow:
    inset 0 2px 4px rgba(0, 0, 0, 0.5),
    inset 0 -1px 0 rgba(255, 255, 255, 0.06);
}

.dpad-dir {
  position: absolute;
  z-index: 2;
  border: none;
  background: transparent;
  color: #c5cad2;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border-radius: 999px;
  transition:
    background 0.15s,
    color 0.15s,
    box-shadow 0.15s;
}

.dpad-dot {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: #7a8089;
  opacity: 1;
}

.key-ok {
  position: absolute;
  left: 50%;
  top: 50%;
  z-index: 1;
  transform: translate(-50%, -50%);
  width: 36px;
  height: 36px;
  border-radius: 50%;
  border: none;
  padding: 0;
  cursor: pointer;
  background: #23262b;
  box-shadow: 0 0 0 1px #0c0d10;
  transition:
    background 0.15s,
    box-shadow 0.15s,
    color 0.15s;
}

.key-ok:hover,
.key-ok.hover,
.key-ok.active {
  transform: translate(-50%, -50%);
}

.dpad-up {
  left: 50%;
  top: 2px;
  transform: translateX(-50%);
  width: 38px;
  height: 24px;
}
.dpad-up.active,
.dpad-up.hover {
  transform: translateX(-50%);
}

.dpad-down {
  left: 50%;
  bottom: 2px;
  transform: translateX(-50%);
  width: 38px;
  height: 24px;
}
.dpad-down.active,
.dpad-down.hover {
  transform: translateX(-50%);
}

.dpad-left {
  left: 2px;
  top: 50%;
  transform: translateY(-50%);
  width: 24px;
  height: 38px;
}
.dpad-left.active,
.dpad-left.hover {
  transform: translateY(-50%);
}

.dpad-right {
  right: 2px;
  top: 50%;
  transform: translateY(-50%);
  width: 24px;
  height: 38px;
}
.dpad-right.active,
.dpad-right.hover {
  transform: translateY(-50%);
}

.mid-row {
  width: 100%;
  display: flex;
  justify-content: space-between;
  padding: 2px 0 0;
}

.lower-grid {
  width: 100%;
  display: grid;
  grid-template-columns: var(--round-key) var(--round-key);
  justify-content: space-between;
  align-items: stretch;
}

.mute-mouse-pill {
  width: var(--round-key);
  border-radius: calc(var(--round-key) / 2);
  overflow: hidden;
  background: #26292f;
  display: flex;
  flex-direction: column;
  height: var(--pill-h);
}

.pill-half {
  flex: 1;
  border: none;
  background: transparent;
  color: #d8dce2;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  transition:
    background 0.15s,
    box-shadow 0.15s,
    color 0.15s;
}

.lower-right {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  height: var(--pill-h);
  gap: var(--round-gap);
}

.grip {
  height: calc(var(--pill-h) * 0.75);
  width: 100%;
}

.side-vol {
  position: absolute;
  top: calc(var(--dpad-top) + var(--dpad) - var(--vol-h));
  right: 0;
  z-index: 3;
  width: var(--vol-w);
  height: var(--vol-h);
  border-radius: 6px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: #1a1c20;
  box-shadow:
    0 0 0 1px rgba(143, 152, 164, 0.7),
    1px 2px 4px rgba(15, 23, 42, 0.22);
}

.vol-half {
  flex: 1;
  border: none;
  padding: 0;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: #e6e9ee;
  background: #2a2e34;
  transition:
    background 0.15s,
    box-shadow 0.15s,
    color 0.15s;
}

.vol-half + .vol-half {
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

.vol-mark {
  width: 10px;
  height: 10px;
  display: block;
  pointer-events: none;
}
</style>
