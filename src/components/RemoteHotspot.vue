<script setup lang="ts">
/**
 * 扁平示意遥控器（对齐附图键位，无底板背景）
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

const GRID_KEYS = [
  { id: "back", label: "返回" },
  { id: "volume_up", label: "音量+" },
  { id: "home", label: "主页" },
  { id: "volume_down", label: "音量-" },
  { id: "menu", label: "菜单" },
  { id: "tv", label: "TV" },
] as const;

function keyEl(id: string): HTMLElement | null {
  return (
    rootRef.value?.querySelector(`[data-key-id="${id}"]`) as HTMLElement | null
  );
}

defineExpose({ keyEl, rootRef });
</script>

<template>
  <div ref="rootRef" class="remote-schematic" aria-label="小米遥控器示意">
    <div class="row top-row">
      <button
        type="button"
        class="key-cap-power-voice"
        
        data-key-id="power"
        :class="{
          active: selectedId === 'power',
          hover: hoverId === 'power',
        }"
        @mouseenter="emit('hover', 'power')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'power')"
      >
        电源
      </button>
      <button
        type="button"
        class="key-cap-power-voice"
        data-key-id="mic"
        :class="{
          active: selectedId === 'mic',
          hover: hoverId === 'mic',
        }"
        @mouseenter="emit('hover', 'mic')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'mic')"
      >
        语音
      </button>
    </div>

    <div class="dpad">
      <button
        type="button"
        class="key-cap key-oval dpad-up"
        data-key-id="up"
        :class="{ active: selectedId === 'up', hover: hoverId === 'up' }"
        @mouseenter="emit('hover', 'up')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'up')"
      >
        上
      </button>
      <button
        type="button"
        class="key-cap key-oval dpad-left"
        data-key-id="left"
        :class="{ active: selectedId === 'left', hover: hoverId === 'left' }"
        @mouseenter="emit('hover', 'left')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'left')"
      >
        左
      </button>
      <button
        type="button"
        class="key-cap key-ok"
        data-key-id="ok"
        :class="{ active: selectedId === 'ok', hover: hoverId === 'ok' }"
        @mouseenter="emit('hover', 'ok')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'ok')"
      >
        确定
      </button>
      <button
        type="button"
        class="key-cap key-oval dpad-right"
        data-key-id="right"
        :class="{ active: selectedId === 'right', hover: hoverId === 'right' }"
        @mouseenter="emit('hover', 'right')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'right')"
      >
        右
      </button>
      <button
        type="button"
        class="key-cap key-oval dpad-down"
        data-key-id="down"
        :class="{ active: selectedId === 'down', hover: hoverId === 'down' }"
        @mouseenter="emit('hover', 'down')"
        @mouseleave="emit('hover', null)"
        @click="emit('select', 'down')"
      >
        下
      </button>
    </div>

    <div class="grid-keys">
      <button
        v-for="k in GRID_KEYS"
        :key="k.id"
        type="button"
        class="key-cap"
        :data-key-id="k.id"
        :class="{
          active: selectedId === k.id,
          hover: hoverId === k.id,
        }"
        @mouseenter="emit('hover', k.id)"
        @mouseleave="emit('hover', null)"
        @click="emit('select', k.id)"
      >
        {{ k.label }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.remote-schematic {
  width: 168px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  user-select: none;
  background: transparent;
  padding: 0;
  border: 1px solid rgb(49, 49, 49);
  border-radius: 10px;
  padding: 20px 8px;
}

.row {
  display: flex;
  justify-content: space-between;
  width: 100%;
  gap: 16px;
   
}

.key-cap {
  min-width: 44px;
  height: 44px;
  padding: 0 6px;
  border-radius: 50%;
  border: 1.5px solid #54575c;
  background: #343639;
  color: #f8fafc;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s, border-color 0.15s, box-shadow 0.15s, transform 0.12s;
}

.key-cap-power-voice{
  min-width: 44px;
  height: 44px;
  padding: 0 6px;
  border-radius: 50%;
  border: 1.5px solid #54575c;
  background: #f5f5f9;
  color: #333;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s, border-color 0.15s, box-shadow 0.15s, transform 0.12s;
}

.key-cap:hover,
.key-cap.hover {
  background: #3b82f6;
  border-color: #93c5fd;
  transform: scale(1.04);
  
}

.key-cap.active {
  background: #2564eb;
  border-color: #60a5fa;
  box-shadow: 0 0 0 2px rgba(37, 99, 235, 0.28);
}

.key-cap-power-voice:hover,
.key-cap-power-voice.hover {
 color: #ffffff !important;
  background: #3b82f6;
  border-color: #93c5fd;
  transform: scale(1.04);
}

.key-cap-power-voice.active {
  color: #ffffff !important;
  background: #2564eb;
  border-color: #60a5fa;
  box-shadow: 0 0 0 2px rgba(37, 99, 235, 0.28);
}



.dpad {
  position: relative;
  width: 128px;
  height: 128px;
  border-radius: 50%;
  border: 1px solid #cbd5e1;
  background: #333;
}

.dpad .key-cap {
  position: absolute;
  min-width: 36px;
  height: 32px;
  font-size: 11px;
}

.dpad .key-ok {
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: 40px;
  height: 40px;
  min-width: 40px;
  min-height: 40px;
  padding: 0;
  border-radius: 50%;
  box-sizing: border-box;
  aspect-ratio: 1;
  line-height: 1;
}
.dpad .key-ok.active,
.dpad .key-ok.hover {
  transform: translate(-50%, -50%) scale(1.04);
}

.key-oval {
  border-radius: 999px;
}

.dpad-up {
  left: 50%;
  top: 4px;
  transform: translateX(-50%);
  width: 38px;
}
.dpad-up.active,
.dpad-up.hover {
  transform: translateX(-50%) scale(1.04);
}

.dpad-down {
  left: 50%;
  bottom: 4px;
  transform: translateX(-50%);
  width: 38px;
}
.dpad-down.active,
.dpad-down.hover {
  transform: translateX(-50%) scale(1.04);
}

.dpad-left {
  left: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 38px;
  height: 38px;
}
.dpad-left.active,
.dpad-left.hover {
  transform: translateY(-50%) scale(1.04);
}

.dpad-right {
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 38px;
  height: 38px;
}
.dpad-right.active,
.dpad-right.hover {
  transform: translateY(-50%) scale(1.04);
}

.grid-keys {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 16px;
  width: 100%;
  justify-items: center;
}

.grid-keys .key-cap {
  width: 48px;
  height: 48px;
}
</style>
