<script setup lang="ts">
import { computed } from "vue";
import {
  CABLE_VOL_DB_HIGH,
  CABLE_VOL_DB_LOW,
  CABLE_VOL_DB_MIN,
  CABLE_VOL_DB_MAX,
  CABLE_VOL_TICKS,
  cableDbToPct,
  cableLevelToDb,
  cableZoneForDb,
} from "../utils/cableVolMeter";

const props = defineProps<{
  level: number;
  disabled?: boolean;
  active?: boolean;
}>();

const db = computed(() => cableLevelToDb(props.level));
const markerPct = computed(() => cableDbToPct(db.value));
const zone = computed(() =>
  props.disabled ? "idle" : cableZoneForDb(db.value)
);

const shellState = computed(() => {
  if (props.disabled) return "disabled";
  if (props.active) return "active";
  return "idle";
});

const zoneLowWidth = cableDbToPct(CABLE_VOL_DB_LOW);
const zoneOkWidth = cableDbToPct(CABLE_VOL_DB_HIGH) - zoneLowWidth;
const zoneHighWidth = 100 - cableDbToPct(CABLE_VOL_DB_HIGH);

const ariaLabel = computed(() => {
  if (props.disabled) return "虚拟声卡音量标尺（未就绪）";
  if (!props.active) return "虚拟声卡音量标尺：无信号";
  return `虚拟声卡音量 ${Math.round(db.value)} dBFS`;
});
</script>

<template>
  <div
    class="cable-vol-ruler"
    :class="[`zone-${zone}`, `shell-${shellState}`]"
    role="meter"
    :aria-valuenow="Math.round(db)"
    :aria-valuemin="CABLE_VOL_DB_MIN"
    :aria-valuemax="CABLE_VOL_DB_MAX"
    :aria-label="ariaLabel"
  >
    <div class="ruler-track">
      <div class="ruler-zones" aria-hidden="true">
        <span class="ruler-zone zone-low" :style="{ width: `${zoneLowWidth}%` }" />
        <span class="ruler-zone zone-ok" :style="{ width: `${zoneOkWidth}%` }" />
        <span class="ruler-zone zone-high" :style="{ width: `${zoneHighWidth}%` }" />
      </div>
      <span
        v-for="tick in CABLE_VOL_TICKS"
        :key="tick"
        class="ruler-tick"
        :style="{ left: `${cableDbToPct(tick)}%` }"
        aria-hidden="true"
      />
      <span
        class="ruler-marker"
        :style="{ left: `${markerPct}%` }"
        aria-hidden="true"
      />
    </div>
    <div class="ruler-labels" aria-hidden="true">
      <span
        v-for="(tick, index) in CABLE_VOL_TICKS"
        :key="`lbl-${tick}`"
        class="ruler-label"
        :class="{
          'at-start': index === 0,
          'at-end': index === CABLE_VOL_TICKS.length - 1,
        }"
        :style="{ left: `${cableDbToPct(tick)}%` }"
      >{{ tick }}</span>
    </div>
  </div>
</template>

<style scoped>
.cable-vol-ruler {
  width: 100%;
  height: 28px;
  padding: 0 4px 2px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  border-radius: 4px;
  background: #f1f5f9;
  border: 1px solid var(--border, #e2e8f0);
  overflow: hidden;
}

.cable-vol-ruler.shell-active {
  background: #ecfdf5;
  border-color: #bbf7d0;
}

.cable-vol-ruler.shell-idle .ruler-zone,
.cable-vol-ruler.shell-disabled .ruler-zone {
  opacity: 0;
}

.ruler-track {
  position: relative;
  height: 10px;
  border-radius: 3px;
  border: 1px solid transparent;
  background: transparent;
  overflow: visible;
}

.cable-vol-ruler.shell-idle .ruler-track,
.cable-vol-ruler.shell-disabled .ruler-track {
  border-color: transparent;
  background: transparent;
}

.cable-vol-ruler.shell-active .ruler-track {
  border-color: rgba(187, 247, 208, 0.9);
  background: rgba(255, 255, 255, 0.35);
}

.ruler-zones {
  position: absolute;
  inset: 0;
  display: flex;
  border-radius: 3px;
  overflow: hidden;
}

.ruler-zone {
  height: 100%;
  opacity: 0.35;
}

.zone-low {
  background: #fde68a;
}

.zone-ok {
  background: #bbf7d0;
}

.zone-high {
  background: #fecaca;
}

.ruler-tick {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  margin-left: -0.5px;
  background: rgba(100, 116, 139, 0.35);
  pointer-events: none;
}

.ruler-marker {
  position: absolute;
  top: -2px;
  bottom: -2px;
  width: 2px;
  margin-left: -1px;
  border-radius: 1px;
  background: #94a3b8;
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.85);
  transition: left 70ms linear, background-color 120ms ease;
  pointer-events: none;
}

.shell-active.zone-low .ruler-marker {
  background: #ca8a04;
}

.shell-active.zone-ok .ruler-marker {
  background: #16a34a;
}

.shell-active.zone-high .ruler-marker {
  background: #dc2626;
}

.ruler-labels {
  position: relative;
  height: 14px;
  margin-top: 2px;
}

.ruler-label {
  position: absolute;
  transform: translateX(-50%);
  font-size: 9px;
  line-height: 1;
  color: #94a3b8;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.ruler-label.at-start {
  transform: translateX(0);
}

.ruler-label.at-end {
  transform: translateX(-100%);
}
</style>
