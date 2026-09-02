<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  level: number | null;
}>();

const clampedLevel = computed(() => {
  if (props.level == null || Number.isNaN(props.level)) return null;
  return Math.max(0, Math.min(100, Math.round(props.level)));
});

const fillWidth = computed(() =>
  clampedLevel.value == null ? "0%" : `${clampedLevel.value}%`
);

const tone = computed(() => {
  const level = clampedLevel.value;
  if (level == null) return "unknown";
  if (level < 10) return "red";
  if (level < 25) return "yellow";
  return "green";
});

const ariaLabel = computed(() => {
  if (clampedLevel.value == null) return "电量未知";
  return `电量 ${clampedLevel.value}%`;
});
</script>

<template>
  <span
    class="battery-icon"
    :class="`is-${tone}`"
    role="img"
    :aria-label="ariaLabel"
  >
    <span class="battery-body">
      <span class="battery-track">
        <span class="battery-fill" :style="{ width: fillWidth }" />
      </span>
    </span>
    <span class="battery-cap" aria-hidden="true" />
  </span>
</template>

<style scoped>
.battery-icon {
  display: inline-flex;
  align-items: center;
  gap: 1px;
  flex-shrink: 0;
  color: #94a3b8;
}

.battery-body {
  box-sizing: border-box;
  position: relative;
  width: 24px;
  height: 12px;
  border: 1px solid currentColor;
  border-radius: 2px;
  flex-shrink: 0;
}

.battery-track {
  position: absolute;
  top: 1px;
  left: 1px;
  right: 1px;
  bottom: 1px;
  border-radius: 1px;
  overflow: hidden;
}

.battery-fill {
  display: block;
  height: 100%;
  max-width: 100%;
  border-radius: 1px;
  background: currentColor;
  transition: width 0.25s ease, background-color 0.25s ease;
}

.battery-cap {
  width: 2px;
  height: 5px;
  border-radius: 0 1px 1px 0;
  background: currentColor;
}

.battery-icon.is-green {
  color: #16a34a;
}

.battery-icon.is-yellow {
  color: #ca8a04;
}

.battery-icon.is-red {
  color: #dc2626;
}

.battery-icon.is-unknown {
  color: #94a3b8;
}

.battery-icon.is-unknown .battery-fill {
  width: 0 !important;
}
</style>
