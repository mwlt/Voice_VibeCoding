<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  keyId: string;
}>();

const isDpadDir = computed(() =>
  ["up", "down", "left", "right"].includes(props.keyId)
);

const isVolume = computed(() =>
  props.keyId === "volume_up" || props.keyId === "volume_down"
);
</script>

<template>
  <span
    class="remote-key-icon"
    :class="{
      'shape-vol': isVolume,
      'shape-ok': keyId === 'ok',
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
      v-else-if="keyId === 'mic'"
      class="key-glyph key-glyph-mic"
      viewBox="0 0 24 24"
    >
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

    <span v-else-if="isDpadDir" class="dpad-mini" :class="`dpad-${keyId}`">
      <span class="dpad-dot" />
    </span>

    <span v-else-if="keyId === 'ok'" class="ok-ring" />

    <svg
      v-else-if="keyId === 'back'"
      class="key-glyph"
      viewBox="0 0 24 24"
    >
      <path
        d="M14.2 7 9.2 12l5 5"
        fill="none"
        stroke="currentColor"
        stroke-width="1.65"
        stroke-linecap="round"
        stroke-linejoin="round"
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
      v-else-if="keyId === 'volume_up'"
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
      v-else-if="keyId === 'volume_down'"
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

    <svg
      v-else-if="keyId === 'tv'"
      class="key-glyph key-glyph-tv"
      viewBox="0 0 24 24"
    >
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
        font-weight="500"
        font-family="Segoe UI, system-ui, sans-serif"
        letter-spacing="0.4"
      >
        TV
      </text>
    </svg>
  </span>
</template>

<style scoped>
.remote-key-icon {
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

.remote-key-icon.shape-vol {
  height: 22px;
  border-radius: 10px;
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

.key-glyph-mic {
  width: 13px;
  height: 13px;
}

.key-glyph-menu {
  width: 11px;
  height: 11px;
}

.key-glyph-vol {
  width: 12px;
  height: 12px;
}

.key-glyph-tv {
  width: 14px;
  height: 14px;
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
  background: currentColor;
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
