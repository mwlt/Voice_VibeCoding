<script setup lang="ts">
import { computed } from "vue";
import { t1IsFixedSystemKey } from "../../utils/t1Keys";

const props = defineProps<{
  keyId: string;
}>();

const isDpadDir = computed(() =>
  ["up", "down", "left", "right"].includes(props.keyId)
);

const isVolume = computed(
  () => props.keyId === "vol_plus" || props.keyId === "vol_minus"
);

const isFixedSystem = computed(() => t1IsFixedSystemKey(props.keyId));
</script>

<template>
  <span
    class="t1-key-icon"
    :class="{
      'shape-vol': isVolume,
      'shape-ok': keyId === 'ok',
      'shape-pill': keyId === 'mute' || keyId === 'mouse',
      'icon-fixed': isFixedSystem,
    }"
    aria-hidden="true"
  >
    <svg
      v-if="keyId === 'power'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
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

    <svg
      v-else-if="keyId === 'voice'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
      <circle cx="8.2" cy="14.2" r="2.35" fill="currentColor" />
      <circle cx="13.4" cy="16.1" r="1.55" fill="currentColor" />
      <circle cx="15.9" cy="11.4" r="2.05" fill="currentColor" />
      <circle cx="11.1" cy="8.6" r="2.9" fill="currentColor" />
    </svg>

    <span v-else-if="isDpadDir" class="dpad-mini" :class="`dpad-${keyId}`">
      <span class="dpad-dot" />
    </span>

    <span v-else-if="keyId === 'ok'" class="ok-ring" />

    <svg
      v-else-if="keyId === 'delete'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
      <path
        d="M11.1 6.2 4.4 12l6.7 5.8v-3.4H19.8V9.6H11.1V6.2z"
        fill="currentColor"
      />
    </svg>

    <svg
      v-else-if="keyId === 'home'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
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

    <svg
      v-else-if="keyId === 'menu'"
      class="key-glyph key-glyph-menu"
      viewBox="0 0 24 24"
    >
      <path
        d="M7 8.2h10M7 12h10M7 15.8h10"
        fill="none"
        stroke="currentColor"
        stroke-width="1.55"
        stroke-linecap="round"
      />
    </svg>

    <svg
      v-else-if="keyId === 'mute'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
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

    <svg
      v-else-if="keyId === 'mouse'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
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

    <svg
      v-else-if="keyId === 'vol_plus'"
      class="key-glyph key-glyph-vol"
      viewBox="0 0 24 24"
    >
      <path
        d="M12 7.5v9M7.5 12h9"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
      />
    </svg>

    <svg
      v-else-if="keyId === 'vol_minus'"
      class="key-glyph key-glyph-vol"
      viewBox="0 0 24 24"
    >
      <path
        d="M7.5 12h9"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
      />
    </svg>
  </span>
</template>

<style scoped>
.t1-key-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  flex-shrink: 0;
  box-sizing: border-box;
  background: #fff;
  color: #64748b;
  border: 1px solid #cbd5e1;
}

.t1-key-icon.icon-fixed {
  color: #6b8499;
  border-color: #9aafc2;
  background: rgba(107, 132, 153, 0.14);
}

.t1-key-icon.shape-vol {
  height: 22px;
  border-radius: 10px;
}

.t1-key-icon.shape-pill {
  border-radius: 8px;
}

.ok-ring {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  border: 1px solid #94a3b8;
  background: #fff;
}

.key-glyph {
  width: 12px;
  height: 12px;
  display: block;
  pointer-events: none;
}

.key-glyph-menu {
  width: 11px;
  height: 11px;
}

.key-glyph-vol {
  width: 12px;
  height: 12px;
}

.dpad-mini {
  position: relative;
  width: 100%;
  height: 100%;
  border-radius: 50%;
}

.dpad-dot {
  position: absolute;
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: #7a8089;
}

.dpad-up .dpad-dot {
  top: 4px;
  left: 50%;
  transform: translateX(-50%);
}

.dpad-down .dpad-dot {
  bottom: 4px;
  left: 50%;
  transform: translateX(-50%);
}

.dpad-left .dpad-dot {
  left: 4px;
  top: 50%;
  transform: translateY(-50%);
}

.dpad-right .dpad-dot {
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
}
</style>
