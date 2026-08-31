<script setup lang="ts">
/**
 * 示意遥控器：对齐小米遥控器 2 Pro 实物比例与层次
 */
import { ref } from "vue";

defineProps<{
  selectedId?: string | null;
  hoverId?: string | null;
}>();

const emit = defineEmits<{
  select: [buttonId: string];
  hover: [buttonId: string | null];
}>();

const rootRef = ref<HTMLElement | null>(null);

function keyEl(id: string): HTMLElement | null {
  return (
    rootRef.value?.querySelector(`[data-key-id="${id}"]`) as HTMLElement | null
  );
}

defineExpose({ keyEl, rootRef });
</script>

<template>
  <div ref="rootRef" class="remote-schematic" aria-label="小米遥控器示意">
    <div class="top-row">
      <div class="well well-sm">
        <button
          type="button"
          class="key-cap key-cap-sm"
          data-key-id="power"
          aria-label="电源"
          :class="{
            active: selectedId === 'power',
            hover: hoverId === 'power',
          }"
          @mouseenter="emit('hover', 'power')"
          @mouseleave="emit('hover', null)"
          @click="emit('select', 'power')"
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
      <div class="well well-sm">
        <button
          type="button"
          class="key-cap key-cap-sm"
          data-key-id="mic"
          aria-label="语音"
          :class="{
            active: selectedId === 'mic',
            hover: hoverId === 'mic',
          }"
          @mouseenter="emit('hover', 'mic')"
          @mouseleave="emit('hover', null)"
          @click="emit('select', 'mic')"
        >
          <svg class="key-icon key-icon-mic" viewBox="0 0 24 24" aria-hidden="true">
            <rect
              x="9.4"
              y="4.5"
              width="5.2"
              height="8.6"
              rx="2.6"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
            />
            <path
              d="M7.4 12.4a4.6 4.6 0 0 0 9.2 0"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>
    </div>

    <div class="well well-dpad">
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
          :class="{ active: selectedId === 'left', hover: hoverId === 'left' }"
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
          :class="{ active: selectedId === 'right', hover: hoverId === 'right' }"
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
          :class="{ active: selectedId === 'down', hover: hoverId === 'down' }"
          @mouseenter="emit('hover', 'down')"
          @mouseleave="emit('hover', null)"
          @click="emit('select', 'down')"
        >
          <span class="dpad-dot" />
        </button>
      </div>
    </div>

    <div class="lower-keys">
      <div class="lower-left">
        <div class="well well-md">
          <button
            type="button"
            class="key-cap key-cap-md"
            data-key-id="back"
            aria-label="返回"
            :class="{
              active: selectedId === 'back',
              hover: hoverId === 'back',
            }"
            @mouseenter="emit('hover', 'back')"
            @mouseleave="emit('hover', null)"
            @click="emit('select', 'back')"
          >
            <svg class="key-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M14.2 7 9.2 12l5 5"
                fill="none"
                stroke="currentColor"
                stroke-width="1.65"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </button>
        </div>
        <div class="well well-md">
          <button
            type="button"
            class="key-cap key-cap-md"
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
        </div>
        <div class="well well-md">
          <button
            type="button"
            class="key-cap key-cap-md"
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
            <svg class="key-icon key-icon-menu" viewBox="0 0 24 24" aria-hidden="true">
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

      <div class="lower-right">
        <div class="well well-vol">
          <div class="vol-rocker" role="group" aria-label="音量">
            <button
              type="button"
              class="vol-half"
              data-key-id="volume_up"
              aria-label="音量+"
              :class="{
                active: selectedId === 'volume_up',
                hover: hoverId === 'volume_up',
              }"
              @mouseenter="emit('hover', 'volume_up')"
              @mouseleave="emit('hover', null)"
              @click="emit('select', 'volume_up')"
            >
              <span class="vol-hit">
                <svg class="key-icon key-icon-vol" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M12 7.5v9M7.5 12h9"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.6"
                    stroke-linecap="round"
                  />
                </svg>
              </span>
            </button>
            <button
              type="button"
              class="vol-half"
              data-key-id="volume_down"
              aria-label="音量-"
              :class="{
                active: selectedId === 'volume_down',
                hover: hoverId === 'volume_down',
              }"
              @mouseenter="emit('hover', 'volume_down')"
              @mouseleave="emit('hover', null)"
              @click="emit('select', 'volume_down')"
            >
              <span class="vol-hit">
                <svg class="key-icon key-icon-vol" viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M7.5 12h9"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.6"
                    stroke-linecap="round"
                  />
                </svg>
              </span>
            </button>
          </div>
        </div>
        <div class="well well-md">
          <button
            type="button"
            class="key-cap key-cap-md"
            data-key-id="tv"
            aria-label="TV"
            :class="{
              active: selectedId === 'tv',
              hover: hoverId === 'tv',
            }"
            @mouseenter="emit('hover', 'tv')"
            @mouseleave="emit('hover', null)"
            @click="emit('select', 'tv')"
          >
            <svg class="key-icon key-icon-tv" viewBox="0 0 24 24" aria-hidden="true">
              <rect
                x="4"
                y="6.5"
                width="16"
                height="11"
                rx="2.8"
                fill="none"
                stroke="currentColor"
                stroke-width="1.4"
              />
              <text
                x="12"
                y="14.05"
                text-anchor="middle"
                fill="currentColor"
                font-size="6.4"
                font-weight="600"
                font-family="Segoe UI, system-ui, sans-serif"
                letter-spacing="0.4"
              >
                TV
              </text>
            </svg>
          </button>
        </div>
      </div>
    </div>

    <div class="remote-footer" aria-hidden="true">
      <svg class="nfc-mark" viewBox="0 0 24 24">
        <rect
          x="5.2"
          y="5.2"
          width="13.6"
          height="13.6"
          rx="2.2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
        />
        <path
          d="M9.2 15.2V8.8h2.1c1.55 0 2.5.85 2.5 2.15 0 .95-.55 1.65-1.4 1.95L15 15.2h-1.55l-2.05-2.15H10.5V15.2H9.2zm1.3-3.45h.85c.7 0 1.1-.35 1.1-.95s-.4-.95-1.1-.95h-.85v1.9z"
          fill="currentColor"
        />
      </svg>
      <span class="brand">xiaomi</span>
    </div>
  </div>
</template>

<style scoped>
.remote-schematic {
  --body-hi: #f6f7f9;
  --body-mid: #eef0f3;
  --body-lo: #e4e7ec;
  --well: #9aa0a8;
  --key: #1c1d1f;
  --key-hi: #2a2b2e;
  --ink: #d7dbe0;
  /* 宽度收窄：圆盘左右边距约为原 20px 的 1/3 → 7px；按键尺寸不变 */
  width: 130px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  user-select: none;
  padding: 18px 6px 16px;
  border-radius: 14px;
  border: 1px solid #c5cad1;
  background:
    linear-gradient(90deg, rgba(0, 0, 0, 0.03) 0%, transparent 14%, transparent 86%, rgba(0, 0, 0, 0.03) 100%),
    linear-gradient(180deg, var(--body-hi) 0%, var(--body-mid) 42%, var(--body-lo) 100%);
  box-shadow: none;
}

/* 与下半回退 / 音量列同宽同边距，仅水平对齐，不改纵向间距 */
.top-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  column-gap: 18px;
  width: 100%;
  padding: 0 4px;
  justify-items: center;
  align-items: start;
}

/* 按键外圈井壁：默认无银圈、无阴影（避免虚边不对称） */
.well {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  background: transparent;
  box-shadow: none;
}

.well-sm {
  width: 32px;
  height: 32px;
  padding: 0;
}

.well-md {
  width: 40px;
  height: 40px;
  padding: 0;
}

.well-vol {
  width: 40px;
  height: 88px;
  padding: 0;
  border-radius: 20px;
}

.well-dpad {
  width: 116px;
  height: 116px;
  padding: 0;
  border-radius: 50%;
}

.key-icon {
  width: 18px;
  height: 18px;
  display: block;
  pointer-events: none;
}

.key-icon-menu {
  width: 17px;
  height: 17px;
}

.key-icon-vol {
  width: 18px;
  height: 18px;
}

.key-icon-tv {
  width: 21px;
  height: 21px;
}

/* 一般圆键：扁平实色，无外圈虚边 */
.key-cap {
  width: 100%;
  height: 100%;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: #1c1d1f;
  color: var(--ink);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  box-shadow: none;
  transition: background 0.15s, box-shadow 0.15s, color 0.15s, border-color 0.15s;
}

/* 电源 / 语音：无阴影，只用深灰圆边框 */
.key-cap-sm {
  background: #f0f2f5;
  color: #1a1b1d;
  border: 1px solid #5c6370;
  box-shadow: none;
}

.key-cap-sm .key-icon {
  width: 18px;
  height: 18px;
}

.key-cap-sm .key-icon-mic {
  width: 19px;
  height: 19px;
}

/* 选中 / 悬停：扁平蓝 + 淡蓝同色边高亮，无凸起阴影 */
.key-cap:hover,
.key-cap.hover,
.dpad-dir:hover,
.dpad-dir.hover,
.key-ok:hover,
.key-ok.hover {
  color: #eaf2ff;
  background: rgba(37, 99, 235, 0.9);
  box-shadow: inset 0 0 0 1.5px rgba(147, 197, 253, 0.95);
}

.key-cap.active,
.dpad-dir.active,
.key-ok.active {
  color: #eaf2ff;
  background: rgba(37, 99, 235, 0.9);
  box-shadow: inset 0 0 0 1.5px rgba(147, 197, 253, 0.95);
}

.key-cap-sm:hover,
.key-cap-sm.hover,
.key-cap-sm.active {
  color: #eaf2ff;
  border-color: #93c5fd;
  box-shadow: none;
}

/* 方向大环：扁平实色 */
.dpad {
  position: relative;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  background: #1c1d1f;
  box-shadow: none;
  overflow: hidden;
}

.dpad-dir {
  position: absolute;
  z-index: 2;
  border: none;
  background: transparent;
  color: #c9ced4;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border-radius: 999px;
  transition: background 0.15s, color 0.15s, box-shadow 0.15s;
}

.dpad-dot {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: none;
  opacity: 0.85;
}

/* 确定键：与方向环同色分割，仅等宽细线 */
.key-ok {
  position: absolute;
  left: 50%;
  top: 50%;
  z-index: 1;
  transform: translate(-50%, -50%);
  width: 44px;
  height: 44px;
  border-radius: 50%;
  border: none;
  padding: 0;
  cursor: pointer;
  background: #242628;
  box-shadow: 0 0 0 1px #0f1011;
  transition: background 0.15s, box-shadow 0.15s, color 0.15s;
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
  width: 46px;
  height: 28px;
}
.dpad-up.active,
.dpad-up.hover {
  transform: translateX(-50%);
}

.dpad-down {
  left: 50%;
  bottom: 2px;
  transform: translateX(-50%);
  width: 46px;
  height: 28px;
}
.dpad-down.active,
.dpad-down.hover {
  transform: translateX(-50%);
}

.dpad-left {
  left: 2px;
  top: 50%;
  transform: translateY(-50%);
  width: 28px;
  height: 46px;
}
.dpad-left.active,
.dpad-left.hover {
  transform: translateY(-50%);
}

.dpad-right {
  right: 2px;
  top: 50%;
  transform: translateY(-50%);
  width: 28px;
  height: 46px;
}
.dpad-right.active,
.dpad-right.hover {
  transform: translateY(-50%);
}

.lower-keys {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 18px;
  width: 100%;
  align-items: start;
  justify-items: center;
  padding: 0 4px;
}

.lower-left,
.lower-right {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

/* 音量条：扁平实色，无虚边 */
.vol-rocker {
  width: 100%;
  height: 100%;
  border-radius: 18px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: #1c1d1f;
  box-shadow: none;
}

.vol-half {
  flex: 1;
  border: none;
  background: transparent;
  color: var(--ink);
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}

.vol-half + .vol-half {
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

/* 音量 ±：圆形选区，默认无阴影 */
.vol-hit {
  width: 34px;
  height: 34px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: inherit;
  box-shadow: none;
  transition: background 0.15s, box-shadow 0.15s, color 0.15s;
}

.vol-hit .key-icon-vol {
  width: 20px;
  height: 20px;
}

.vol-half:hover .vol-hit,
.vol-half.hover .vol-hit {
  color: #eaf2ff;
  background: rgba(37, 99, 235, 0.9);
  box-shadow: inset 0 0 0 1.5px rgba(147, 197, 253, 0.95);
}

.vol-half.active .vol-hit {
  color: #eaf2ff;
  background: rgba(37, 99, 235, 0.9);
  box-shadow: inset 0 0 0 1.5px rgba(147, 197, 253, 0.95);
}

.remote-footer {
  margin-top: 6px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  color: #7a8088;
}

.nfc-mark {
  width: 14px;
  height: 14px;
  opacity: 0.7;
}

.brand {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: lowercase;
  font-family: "Segoe UI", system-ui, sans-serif;
  color: #6d737b;
  opacity: 0.85;
}
</style>
