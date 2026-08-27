<script setup lang="ts">
import { onMounted, onUnmounted, computed, ref, nextTick, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { useBridgeStore } from "../stores/bridge";
import { useConfigStore } from "../stores/config";
import DeviceStatus from "../components/DeviceStatus.vue";
import KeyMappingStage from "../components/KeyMappingStage.vue";
import wechatImeHotkeysImg from "../assets/guides/wechat-ime-hotkeysV2.png";
import doubaoImeHotkeysImg from "../assets/guides/doubao.png";
import { vkDisplayName } from "../utils/vkDisplay";
import {
  applyImePresetConfig,
  getPresetsForTab,
  IME_FAQ,
  IME_PRESETS,
  QIANWEN_GUIDE,
  QIANWEN_PRESET_IDS,
  listImeTabs,
  type ImePresetDefinition,
  type ImePresetId,
  type ImeTabId,
} from "../utils/imePreset";

const bridge = useBridgeStore();
const configStore = useConfigStore();
const type = "xiaomi" as const;

const device = computed(() => bridge.devices[type]);
const config = computed(() => configStore.configs[type]);
const configLoadState = computed(() => configStore.loadStates[type]);
const configLoadError = computed(() => configStore.loadErrors[type]);
const configSectionLoading = computed(
  () => configLoadState.value === "pending" || configLoadState.value === "loading"
);

interface HostStatusItem {
  id: string;
  label: string;
  state_label: string;
  tone: string;
}

interface HostStatus {
  bridge_alive: boolean;
  audio_alive: boolean;
  cable_ready: boolean;
  winuhid_ready?: boolean;
  atvv_ok?: boolean;
  status_text: string;
  detail: string;
  tone: string;
  items: HostStatusItem[];
}

const restarting = ref(false);
const voiceRepairing = ref(false);
const winuhidRepairing = ref(false);
const atvvRepairing = ref(false);
const showVoiceChoice = ref(false);
const showWinuhidChoice = ref(false);
const voiceChoiceMsg = ref("");
const winuhidChoiceMsg = ref("");
type WinuhidDownloadPhase = "idle" | "downloading" | "complete" | "error";
const winuhidDownloadPhase = ref<WinuhidDownloadPhase>("idle");
const winuhidDownloadProgress = ref<{
  downloaded: number;
  total?: number | null;
  percent?: number | null;
} | null>(null);
const winuhidDownloadMessage = ref("");
const winuhidZipDefaultName = ref("WinUHid_Manual.zip");
const showVoiceReboot = ref(false);
const voiceRebootMsg = ref("");
const showLogModal = ref(false);
const showSetupTips = ref(false);
const setupApplyHint = ref("");
const setupImeTab = ref<ImeTabId>("wechat");
const imeTabs = listImeTabs();
const imeFaq = IME_FAQ;
const qianwenGuide = QIANWEN_GUIDE;
const qianwenPresets = QIANWEN_PRESET_IDS.map((id) => IME_PRESETS[id]);

/** 设为 true 可恢复「触发模式」下拉（后端当前固定为按住语义，PR #8） */
const SHOW_VOICE_TRIGGER_MODE = false;

const activeImePresets = computed(() => getPresetsForTab(setupImeTab.value));
const logText = ref("");
const logPath = ref("");
const logLoading = ref(false);
const logCopyHint = ref("");

type BleMeterState = "idle" | "session" | "receiving";
interface VoiceMeterSnapshot {
  bleState: BleMeterState;
  bleLevel: number;
  waveform: number[];
  cableActive: boolean;
  cableLevel: number;
  atvvOk: boolean;
}

const voiceMeter = ref<VoiceMeterSnapshot>({
  bleState: "idle",
  bleLevel: 0,
  waveform: Array(28).fill(0),
  cableActive: false,
  cableLevel: 0,
  atvvOk: false,
});

/** 「按键映射」标题旁：最近一次 按下/抬起 + 遥控键：映射 */
const lastMappingFlash = ref<{
  seq: number;
  phase: "down" | "up";
  remote: string;
  mapped: string | null;
} | null>(null);
let mappingFlashSeq = 0;
let mappingFlashClearTimer: ReturnType<typeof setTimeout> | null = null;

const bleSignalLabel = computed(() => {
  switch (voiceMeter.value.bleState) {
    case "receiving":
      return "接收中";
    case "session":
      return "语音会话";
    default:
      return "无信号";
  }
});

const cableActivityLabel = computed(() =>
  voiceMeter.value.cableActive ? "输送中" : "待命"
);

function applyVoiceMeter(p: Record<string, unknown>) {
  const bleState = (p.bleState ?? p.ble_state ?? "idle") as BleMeterState;
  const waveform = p.waveform as number[] | undefined;
  voiceMeter.value = {
    bleState,
    bleLevel: Number(p.bleLevel ?? p.ble_level ?? 0),
    waveform: Array.isArray(waveform) && waveform.length ? [...waveform] : Array(28).fill(0),
    cableActive: Boolean(p.cableActive ?? p.cable_active ?? false),
    cableLevel: Number(p.cableLevel ?? p.cable_level ?? 0),
    atvvOk: Boolean(p.atvvOk ?? p.atvv_ok ?? false),
  };
}
const showVoiceShortcutTip = ref(false);
const showGainTip = ref(false);
const showTriggerTip = ref(false);
const showRepairTip = ref(false);
const showWinuhidTip = ref(false);
const showAtvvTip = ref(false);
const showRestartTip = ref(false);
const voiceInfoBtn = ref<HTMLElement | null>(null);
const gainInfoBtn = ref<HTMLElement | null>(null);
const triggerInfoBtn = ref<HTMLElement | null>(null);
const repairInfoBtn = ref<HTMLElement | null>(null);
const winuhidInfoBtn = ref<HTMLElement | null>(null);
const atvvInfoBtn = ref<HTMLElement | null>(null);
const restartInfoBtn = ref<HTMLElement | null>(null);
const voiceTipEl = ref<HTMLElement | null>(null);
const gainTipEl = ref<HTMLElement | null>(null);
const triggerTipEl = ref<HTMLElement | null>(null);
const repairTipEl = ref<HTMLElement | null>(null);
const winuhidTipEl = ref<HTMLElement | null>(null);
const atvvTipEl = ref<HTMLElement | null>(null);
const restartTipEl = ref<HTMLElement | null>(null);
const voiceTipStyle = ref<Record<string, string>>({});
const gainTipStyle = ref<Record<string, string>>({});
const triggerTipStyle = ref<Record<string, string>>({});
const repairTipStyle = ref<Record<string, string>>({});
const winuhidTipStyle = ref<Record<string, string>>({});
const atvvTipStyle = ref<Record<string, string>>({});
const restartTipStyle = ref<Record<string, string>>({});
let voiceTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let gainTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let triggerTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let repairTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let winuhidTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let atvvTipCloseTimer: ReturnType<typeof setTimeout> | null = null;
let restartTipCloseTimer: ReturnType<typeof setTimeout> | null = null;

/** 右上 / 右下自动落位，并钳制在视口内 */
function placeInfoTip(
  anchor: HTMLElement | null,
  tip: HTMLElement | null,
  styleRef: typeof voiceTipStyle
) {
  if (!anchor || !tip) return;
  const margin = 8;
  const pad = 8;
  const ar = anchor.getBoundingClientRect();
  const tw = tip.offsetWidth || Math.min(420, window.innerWidth - pad * 2);
  const th = tip.offsetHeight || 120;
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  const spaceBelow = vh - ar.bottom - margin;
  const spaceAbove = ar.top - margin;
  // 优先右下方；下方不够且上方更宽裕则改右上方
  const placeBelow = spaceBelow >= th || spaceBelow >= spaceAbove;

  let top = placeBelow ? ar.bottom + margin : ar.top - th - margin;
  // 右对齐图标右侧（右上/右下）
  let left = ar.right - tw;

  if (left < pad) left = pad;
  if (left + tw > vw - pad) left = Math.max(pad, vw - pad - tw);
  if (top < pad) top = pad;
  if (top + th > vh - pad) top = Math.max(pad, vh - pad - th);

  styleRef.value = {
    position: "fixed",
    top: `${Math.round(top)}px`,
    left: `${Math.round(left)}px`,
    right: "auto",
    bottom: "auto",
    zIndex: "2000",
    visibility: "visible",
    maxWidth: `${Math.min(420, vw - pad * 2)}px`,
  };
}

async function openVoiceTip() {
  if (voiceTipCloseTimer) {
    clearTimeout(voiceTipCloseTimer);
    voiceTipCloseTimer = null;
  }
  voiceTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showVoiceShortcutTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(voiceInfoBtn.value, voiceTipEl.value, voiceTipStyle);
  });
}

function scheduleCloseVoiceTip() {
  if (voiceTipCloseTimer) clearTimeout(voiceTipCloseTimer);
  voiceTipCloseTimer = setTimeout(() => {
    showVoiceShortcutTip.value = false;
  }, 120);
}

function toggleVoiceTip() {
  if (showVoiceShortcutTip.value) {
    showVoiceShortcutTip.value = false;
  } else {
    void openVoiceTip();
  }
}

async function openGainTip() {
  if (gainTipCloseTimer) {
    clearTimeout(gainTipCloseTimer);
    gainTipCloseTimer = null;
  }
  gainTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showGainTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(gainInfoBtn.value, gainTipEl.value, gainTipStyle);
  });
}

function scheduleCloseGainTip() {
  if (gainTipCloseTimer) clearTimeout(gainTipCloseTimer);
  gainTipCloseTimer = setTimeout(() => {
    showGainTip.value = false;
  }, 120);
}

function toggleGainTip() {
  if (showGainTip.value) {
    showGainTip.value = false;
  } else {
    void openGainTip();
  }
}

async function openTriggerTip() {
  if (triggerTipCloseTimer) {
    clearTimeout(triggerTipCloseTimer);
    triggerTipCloseTimer = null;
  }
  triggerTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showTriggerTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(triggerInfoBtn.value, triggerTipEl.value, triggerTipStyle);
  });
}

function scheduleCloseTriggerTip() {
  if (triggerTipCloseTimer) clearTimeout(triggerTipCloseTimer);
  triggerTipCloseTimer = setTimeout(() => {
    showTriggerTip.value = false;
  }, 120);
}

function toggleTriggerTip() {
  if (showTriggerTip.value) {
    showTriggerTip.value = false;
  } else {
    void openTriggerTip();
  }
}

async function openRepairTip() {
  if (repairTipCloseTimer) {
    clearTimeout(repairTipCloseTimer);
    repairTipCloseTimer = null;
  }
  repairTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showRepairTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(repairInfoBtn.value, repairTipEl.value, repairTipStyle);
  });
}

function scheduleCloseRepairTip() {
  if (repairTipCloseTimer) clearTimeout(repairTipCloseTimer);
  repairTipCloseTimer = setTimeout(() => {
    showRepairTip.value = false;
  }, 120);
}

function toggleRepairTip() {
  if (showRepairTip.value) {
    showRepairTip.value = false;
  } else {
    void openRepairTip();
  }
}

async function openWinuhidTip() {
  if (winuhidTipCloseTimer) {
    clearTimeout(winuhidTipCloseTimer);
    winuhidTipCloseTimer = null;
  }
  winuhidTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showWinuhidTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(winuhidInfoBtn.value, winuhidTipEl.value, winuhidTipStyle);
  });
}

function scheduleCloseWinuhidTip() {
  if (winuhidTipCloseTimer) clearTimeout(winuhidTipCloseTimer);
  winuhidTipCloseTimer = setTimeout(() => {
    showWinuhidTip.value = false;
  }, 120);
}

function toggleWinuhidTip() {
  if (showWinuhidTip.value) {
    showWinuhidTip.value = false;
  } else {
    void openWinuhidTip();
  }
}

async function openAtvvTip() {
  if (atvvTipCloseTimer) {
    clearTimeout(atvvTipCloseTimer);
    atvvTipCloseTimer = null;
  }
  atvvTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showAtvvTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(atvvInfoBtn.value, atvvTipEl.value, atvvTipStyle);
  });
}

function scheduleCloseAtvvTip() {
  if (atvvTipCloseTimer) clearTimeout(atvvTipCloseTimer);
  atvvTipCloseTimer = setTimeout(() => {
    showAtvvTip.value = false;
  }, 120);
}

function toggleAtvvTip() {
  if (showAtvvTip.value) {
    showAtvvTip.value = false;
  } else {
    void openAtvvTip();
  }
}

async function openRestartTip() {
  if (restartTipCloseTimer) {
    clearTimeout(restartTipCloseTimer);
    restartTipCloseTimer = null;
  }
  restartTipStyle.value = {
    position: "fixed",
    top: "0px",
    left: "0px",
    visibility: "hidden",
    zIndex: "2000",
  };
  showRestartTip.value = true;
  await nextTick();
  requestAnimationFrame(() => {
    placeInfoTip(restartInfoBtn.value, restartTipEl.value, restartTipStyle);
  });
}

function scheduleCloseRestartTip() {
  if (restartTipCloseTimer) clearTimeout(restartTipCloseTimer);
  restartTipCloseTimer = setTimeout(() => {
    showRestartTip.value = false;
  }, 120);
}

function toggleRestartTip() {
  if (showRestartTip.value) {
    showRestartTip.value = false;
  } else {
    void openRestartTip();
  }
}

let tipViewportRaf: number | null = null;

function onViewportChange() {
  const anyTip =
    showVoiceShortcutTip.value ||
    showGainTip.value ||
    showTriggerTip.value ||
    showRepairTip.value ||
    showWinuhidTip.value ||
    showAtvvTip.value ||
    showRestartTip.value;
  if (!anyTip) return;
  if (tipViewportRaf != null) return;
  tipViewportRaf = requestAnimationFrame(() => {
    tipViewportRaf = null;
    if (showVoiceShortcutTip.value) {
      placeInfoTip(voiceInfoBtn.value, voiceTipEl.value, voiceTipStyle);
    }
    if (showGainTip.value) {
      placeInfoTip(gainInfoBtn.value, gainTipEl.value, gainTipStyle);
    }
    if (showTriggerTip.value) {
      placeInfoTip(triggerInfoBtn.value, triggerTipEl.value, triggerTipStyle);
    }
    if (showRepairTip.value) {
      placeInfoTip(repairInfoBtn.value, repairTipEl.value, repairTipStyle);
    }
    if (showWinuhidTip.value) {
      placeInfoTip(winuhidInfoBtn.value, winuhidTipEl.value, winuhidTipStyle);
    }
    if (showAtvvTip.value) {
      placeInfoTip(atvvInfoBtn.value, atvvTipEl.value, atvvTipStyle);
    }
    if (showRestartTip.value) {
      placeInfoTip(restartInfoBtn.value, restartTipEl.value, restartTipStyle);
    }
  });
}
const host = ref<HostStatus>({
  bridge_alive: false,
  audio_alive: false,
  cable_ready: false,
  winuhid_ready: false,
  atvv_ok: false,
  status_text: "正在启动",
  detail: "",
  tone: "warn",
  items: [
    { id: "cable", label: "虚拟声卡", state_label: "检测中", tone: "warn" },
    { id: "winuhid", label: "虚拟键盘", state_label: "检测中", tone: "warn" },
    { id: "audio", label: "语音路由", state_label: "检测中", tone: "warn" },
    { id: "bridge", label: "按键桥接", state_label: "检测中", tone: "warn" },
  ],
});

/** C1：桥接在跑且 ATVV 未订阅 → 音频信号旁红字 */
const showAtvvFailLabel = computed(
  () => Boolean(host.value.bridge_alive) && !(voiceMeter.value.atvvOk || host.value.atvv_ok)
);

const voiceShortcutEnabled = computed({
  get: () => config.value?.voice_shortcut_enabled !== false,
  set: (v: boolean) => {
    if (!config.value) return;
    config.value.voice_shortcut_enabled = v;
    void persistVoiceSettings();
  },
});

const GAIN_MIN = -12;
const GAIN_MAX = 30;
const GAIN_STEP = 1;

const gainDb = computed({
  get: () => config.value?.gain_db ?? 10,
  set: (v: number | string) => {
    if (!config.value) return;
    const n = typeof v === "number" ? v : Number(v);
    if (Number.isNaN(n)) return;
    config.value.gain_db = Math.min(GAIN_MAX, Math.max(GAIN_MIN, n));
    void persistVoiceSettings();
  },
});

function stepGain(delta: number) {
  gainDb.value = Math.min(GAIN_MAX, Math.max(GAIN_MIN, gainDb.value + delta));
}

function clampGainOnBlur() {
  if (!config.value) return;
  const n = Number(config.value.gain_db);
  if (Number.isNaN(n)) {
    config.value.gain_db = 10;
  } else {
    config.value.gain_db = Math.min(GAIN_MAX, Math.max(GAIN_MIN, n));
  }
  void persistVoiceSettings();
}

async function persistVoiceSettings() {
  if (!config.value) return;
  await configStore.saveConfig(type, { ...config.value });
}

/** 输入法一键预设（微信 / 豆包 / 千问等） */
function isWechatPreset(id: ImePresetId): boolean {
  return id.startsWith("wechat-");
}

function isDoubaoHoldPreset(id: ImePresetId): boolean {
  return id === "doubao-hold";
}

function presetShortcutLabel(preset: ImePresetDefinition): string {
  return preset.shortcutVks.map((vk) => vkDisplayName(vk)).join(" + ");
}

async function applyImePreset(presetId: ImePresetId) {
  if (!config.value) return;
  const definition = IME_PRESETS[presetId];
  if (!definition) return;
  const next = applyImePresetConfig(config.value, presetId);
  config.value.button_bindings = next.button_bindings;
  config.value.voice_hotkey = next.voice_hotkey;
  config.value.voice_shortcut_enabled = true;
  config.value.trigger_mode = next.trigger_mode;
  config.value.voice_release_behavior = next.voice_release_behavior;
  await configStore.saveConfig(type, next);
  setupApplyHint.value = definition.applyHint;
  prependLog(definition.logMessage);
  window.setTimeout(() => {
    if (setupApplyHint.value.startsWith("已应用")) setupApplyHint.value = "";
  }, 4000);
}

let hostPollTimer: ReturnType<typeof setInterval> | null = null;
let devicePollTimer: ReturnType<typeof setInterval> | null = null;

function itemToneClass(tone: string): string {
  if (tone === "ok") return "ok";
  if (tone === "warn") return "warn";
  return "error";
}

interface LogEntry {
  id: number;
  time: string;
  text: string;
}

const logs = ref<LogEntry[]>([]);
const logAreaRef = ref<HTMLElement | null>(null);
let logSeq = 0;
let unlistenKey: UnlistenFn | null = null;
let unlistenMeter: UnlistenFn | null = null;
let unlistenAtvvRepair: UnlistenFn | null = null;
let unlistenAtvvCancel: UnlistenFn | null = null;
let unlistenWinuhidProgress: UnlistenFn | null = null;
let unlistenWinuhidComplete: UnlistenFn | null = null;
let unlistenWinuhidError: UnlistenFn | null = null;

function formatTime(d = new Date()): string {
  return d.toLocaleTimeString("zh-CN", { hour12: false });
}

function prependLog(text: string) {
  logs.value.unshift({
    id: ++logSeq,
    time: formatTime(),
    text,
  });
  if (logs.value.length > 80) {
    logs.value.length = 80;
  }
  nextTick(() => {
    const el = logAreaRef.value;
    if (el) {
      // 最新在顶部，自动滚回顶端
      el.scrollTop = 0;
    }
  });
}

function resolveKeyLabel(buttonId: string): string {
  const aliases = config.value?.button_aliases;
  if (aliases && aliases[buttonId]) return aliases[buttonId];
  const fallback: Record<string, string> = {
    power: "电源",
    volume_up: "音量+",
    volume_down: "音量-",
    up: "上",
    down: "下",
    left: "左",
    right: "右",
    dpad_up: "上",
    dpad_down: "下",
    dpad_left: "左",
    dpad_right: "右",
    ok: "确认",
    back: "返回",
    home: "主页",
    menu: "菜单",
    mic: "语音",
    voice: "语音",
    volume_mute: "静音",
    mute: "静音",
    tv: "TV",
  };
  return fallback[buttonId] || buttonId;
}

function bindingAliases(buttonId: string): string[] {
  switch (buttonId) {
    case "mic":
    case "voice":
      return ["mic", "voice"];
    case "mute":
    case "volume_mute":
      return ["mute", "volume_mute"];
    case "up":
    case "dpad_up":
      return ["up", "dpad_up"];
    case "down":
    case "dpad_down":
      return ["down", "dpad_down"];
    case "left":
    case "dpad_left":
      return ["left", "dpad_left"];
    case "right":
    case "dpad_right":
      return ["right", "dpad_right"];
    default:
      return [buttonId];
  }
}

function resolveMappedActionLabel(buttonId: string): string {
  const bindings = config.value?.button_bindings;
  if (!bindings) return "未绑定";
  let action = bindings[buttonId];
  if (!action) {
    for (const alt of bindingAliases(buttonId)) {
      if (bindings[alt]) {
        action = bindings[alt];
        break;
      }
    }
  }
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

function formatKeyEventLine(
  phase: "down" | "up",
  remoteLabel: string,
  mappedLabel: string | null
): string {
  const phaseLabel = phase === "up" ? "抬起" : "按下";
  if (mappedLabel) {
    return `${phaseLabel} ${remoteLabel}：${mappedLabel}`;
  }
  return `${phaseLabel} ${remoteLabel}`;
}

function showMappingFlash(
  remoteLabel: string,
  mappedLabel: string | null,
  phase: "down" | "up" = "down"
) {
  lastMappingFlash.value = {
    seq: ++mappingFlashSeq,
    phase,
    remote: remoteLabel,
    mapped: mappedLabel,
  };
  if (mappingFlashClearTimer) clearTimeout(mappingFlashClearTimer);
  mappingFlashClearTimer = setTimeout(() => {
    lastMappingFlash.value = null;
    mappingFlashClearTimer = null;
  }, 4500);
}

async function refreshHost() {
  try {
    host.value = await invoke<HostStatus>("get_xiaomi_host_status");
  } catch (e) {
    host.value = {
      bridge_alive: false,
      audio_alive: false,
      cable_ready: false,
      atvv_ok: false,
      status_text: "桥接未运行",
      detail: String(e),
      tone: "error",
      items: [
        { id: "cable", label: "虚拟声卡", state_label: "未知", tone: "error" },
        { id: "winuhid", label: "虚拟键盘", state_label: "未知", tone: "error" },
        { id: "audio", label: "语音路由", state_label: "未知", tone: "error" },
        { id: "bridge", label: "按键桥接", state_label: "未启动", tone: "error" },
      ],
    };
  }
}

async function restartBridge() {
  restarting.value = true;
  try {
    await invoke("restart_xiaomi_bridge");
    await refreshHost();
  } catch (e) {
    host.value = {
      ...host.value,
      status_text: "重启失败",
      detail: String(e),
      tone: "error",
    };
  } finally {
    restarting.value = false;
  }
}

interface AtvvRepairResult {
  phase: string;
  message: string;
  atvvOk: boolean;
  hadConflicts: boolean;
}

async function repairAtvv() {
  if (atvvRepairing.value || restarting.value || voiceRepairing.value) return;
  atvvRepairing.value = true;
  let awaitingClear = false;
  try {
    const result = await invoke<AtvvRepairResult>("repair_xiaomi_atvv", {
      force: false,
    });
    awaitingClear = result.phase === "awaiting_conflict_clear";
    host.value = {
      ...host.value,
      status_text: result.atvvOk
        ? "ATVV 已修复"
        : awaitingClear
          ? "等待清理占用"
          : "ATVV 修复未完成",
      detail: result.message,
      tone: result.atvvOk ? "ok" : awaitingClear ? "warn" : "error",
    };
    if (awaitingClear) {
      return;
    }
    await refreshHost();
  } catch (e) {
    host.value = {
      ...host.value,
      status_text: "ATVV 修复失败",
      detail: String(e),
      tone: "error",
    };
  } finally {
    if (!awaitingClear) {
      atvvRepairing.value = false;
    }
  }
}

async function openLogs() {
  showLogModal.value = true;
  logCopyHint.value = "";
  logLoading.value = true;
  try {
    const result = await invoke<{ path: string; content: string }>("get_app_log");
    logPath.value = result.path || "";
    logText.value = result.content?.trim()
      ? result.content
      : "（暂无日志）";
  } catch (e) {
    logText.value = `读取日志失败: ${e}`;
    logPath.value = "";
  } finally {
    logLoading.value = false;
  }
}

async function copyLog() {
  try {
    await navigator.clipboard.writeText(logText.value || "");
    logCopyHint.value = "已复制";
    setTimeout(() => {
      logCopyHint.value = "";
    }, 1500);
  } catch (e) {
    logCopyHint.value = `复制失败: ${e}`;
  }
}

async function openLogExternally() {
  try {
    await invoke("open_app_log");
  } catch (e) {
    logCopyHint.value = `打开失败: ${e}`;
  }
}

interface VoiceEnvActionResult {
  ok: boolean;
  ready: boolean;
  needsChoice: boolean;
  needsReboot: boolean;
  message: string;
  reportPath?: string | null;
}

function applyVoiceEnvResult(result: VoiceEnvActionResult) {
  host.value = {
    ...host.value,
    detail: result.message,
    tone: result.ready ? "ok" : result.needsReboot ? "warn" : result.ok ? "warn" : "error",
  };
  prependLog(result.message);
  if (result.needsReboot) {
    voiceRebootMsg.value = result.message;
    showVoiceReboot.value = true;
  }
}

async function voiceDetectAndRepair() {
  voiceRepairing.value = true;
  showVoiceChoice.value = false;
  showVoiceReboot.value = false;
  try {
    const result = await invoke<VoiceEnvActionResult>("check_xiaomi_voice_env");
    if (result.needsChoice) {
      voiceChoiceMsg.value = result.message;
      showVoiceChoice.value = true;
      return;
    }
    applyVoiceEnvResult(result);
    await refreshHost();
  } catch (e) {
    const msg = `虚拟声卡检测失败: ${e}`;
    prependLog(msg);
    host.value = { ...host.value, detail: msg, tone: "error" };
  } finally {
    voiceRepairing.value = false;
  }
}

async function chooseVoiceSource(source: "embedded" | "download_page" | "download_zip") {
  voiceRepairing.value = true;
  showVoiceChoice.value = false;
  showVoiceReboot.value = false;
  try {
    const result = await invoke<VoiceEnvActionResult>("repair_xiaomi_voice_env", {
      source,
    });
    applyVoiceEnvResult(result);
    await refreshHost();
  } catch (e) {
    const msg = `语音修复失败: ${e}`;
    prependLog(msg);
    host.value = { ...host.value, detail: msg, tone: "error" };
  } finally {
    voiceRepairing.value = false;
  }
}

interface WinUHidActionResult {
  ok: boolean;
  ready: boolean;
  needsChoice: boolean;
  needsReboot: boolean;
  message: string;
  exportPath?: string | null;
}

function applyWinuhidResult(result: WinUHidActionResult) {
  prependLog(result.message);
  host.value = {
    ...host.value,
    winuhid_ready: result.ready,
    detail: result.message,
    tone: result.ready ? "ok" : result.needsReboot ? "warn" : result.ok ? "warn" : "error",
  };
  if (result.needsReboot) {
    voiceRebootMsg.value = result.message;
    showVoiceReboot.value = true;
  }
  if (result.needsChoice) {
    winuhidChoiceMsg.value = result.message;
    showWinuhidChoice.value = true;
  }
}

async function repairWinUHid() {
  if (winuhidRepairing.value || voiceRepairing.value || atvvRepairing.value || restarting.value) {
    return;
  }
  openWinuhidRepairChoice();
}

async function chooseWinuhidSource(
  source: "embedded" | "embedded_force" | "export" | "download_page" | "download_zip"
) {
  if (source === "download_zip") {
    await startWinuhidZipDownload();
    return;
  }
  winuhidRepairing.value = true;
  showWinuhidChoice.value = false;
  try {
    const result = await invoke<WinUHidActionResult>("repair_xiaomi_winuhid", {
      source,
      force: source === "embedded_force",
    });
    applyWinuhidResult(result);
    await refreshHost();
  } catch (e) {
    const msg = `虚拟键盘处理失败: ${e}`;
    prependLog(msg);
    host.value = { ...host.value, detail: msg, tone: "error" };
    winuhidChoiceMsg.value = msg;
    showWinuhidChoice.value = true;
  } finally {
    winuhidRepairing.value = false;
  }
}

function formatDownloadBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

const winuhidDownloadProgressLabel = computed(() => {
  const p = winuhidDownloadProgress.value;
  if (!p) return "准备下载…";
  const downloaded = formatDownloadBytes(p.downloaded);
  if (p.total && p.total > 0) {
    const pct = p.percent != null ? `（${Math.round(p.percent)}%）` : "";
    return `${downloaded} / ${formatDownloadBytes(p.total)}${pct}`;
  }
  return `已下载 ${downloaded}`;
});

function winuhidDownloadProgressWidth(): string {
  const p = winuhidDownloadProgress.value;
  if (p?.percent != null) return `${Math.min(100, Math.max(0, p.percent))}%`;
  if (winuhidDownloadPhase.value === "complete") return "100%";
  return "0%";
}

function resetWinuhidDownloadState() {
  winuhidDownloadPhase.value = "idle";
  winuhidDownloadProgress.value = null;
  winuhidDownloadMessage.value = "";
}

async function refreshWinuhidZipName() {
  try {
    const status = await invoke<{ downloadZipUrl?: string }>("get_xiaomi_winuhid_status");
    const url = status.downloadZipUrl || "";
    const name = url.split("/").pop();
    if (name) winuhidZipDefaultName.value = name;
  } catch {
    /* ignore */
  }
}

async function startWinuhidZipDownload() {
  if (winuhidDownloadPhase.value === "downloading" || winuhidRepairing.value) return;

  const dest = await save({
    defaultPath: winuhidZipDefaultName.value,
    filters: [{ name: "ZIP 压缩包", extensions: ["zip"] }],
    title: "保存 WinUHid 驱动包",
  });
  if (!dest) return;

  winuhidDownloadPhase.value = "downloading";
  winuhidDownloadProgress.value = { downloaded: 0, total: null, percent: null };
  winuhidDownloadMessage.value = "";

  try {
    await invoke("download_xiaomi_winuhid_zip", { destPath: dest });
  } catch (e) {
    winuhidDownloadPhase.value = "error";
    winuhidDownloadMessage.value = String(e);
    prependLog(`WinUHid 驱动包下载失败: ${e}`);
  }
}

function openWinuhidRepairChoice() {
  resetWinuhidDownloadState();
  winuhidChoiceMsg.value = "请选择修复或安装方式：";
  showWinuhidChoice.value = true;
  void refreshWinuhidZipName();
}

onMounted(async () => {
  prependLog("日志区准备就绪");
  await Promise.all([
    bridge.refreshStatus(type),
    configStore.loadConfig(type),
    refreshHost(),
    invoke("get_xiaomi_voice_meter")
      .then((s) => applyVoiceMeter(s as Record<string, unknown>))
      .catch(() => undefined),
  ]);
  hostPollTimer = setInterval(refreshHost, 1000);
  // 持续拉取设备信息（含电量），避免必须切页才刷新
  devicePollTimer = setInterval(() => {
    void bridge.refreshStatus(type);
  }, 1500);
  window.addEventListener("resize", onViewportChange);
  window.addEventListener("scroll", onViewportChange, true);

  try {
    unlistenKey = await listen<{
      buttonId?: string;
      label?: string;
      message?: string;
      phase?: string;
    }>("xiaomi-key", (event) => {
      const p = event.payload;
      if (p.message) {
        prependLog(p.message);
        if (p.message.startsWith("电量")) {
          void bridge.refreshStatus(type);
        }
        return;
      }
      const id = p.buttonId || "unknown";
      const label = p.label || resolveKeyLabel(id);
      const phase: "down" | "up" = p.phase === "up" ? "up" : "down";
      const isVoice = id === "mic" || id === "voice";
      const voiceMapOn = config.value?.voice_shortcut_enabled !== false;
      // D1：语音映射关闭时只显示按下/抬起，不写映射段
      const lineMapped = isVoice && !voiceMapOn ? null : resolveMappedActionLabel(id);
      showMappingFlash(label, lineMapped, phase);
      prependLog(formatKeyEventLine(phase, label, lineMapped));
    });
  } catch (e) {
    console.warn("listen xiaomi-key failed:", e);
  }

  try {
    unlistenMeter = await listen<Record<string, unknown>>("xiaomi-voice-meter", (event) => {
      applyVoiceMeter(event.payload);
    });
  } catch (e) {
    console.warn("listen xiaomi-voice-meter failed:", e);
  }

  try {
    unlistenAtvvRepair = await listen<{ ok?: boolean; message?: string }>(
      "xiaomi-atvv-repair-result",
      async (event) => {
        const p = event.payload || {};
        host.value = {
          ...host.value,
          status_text: p.ok ? "ATVV 已修复" : "ATVV 修复未完成",
          detail: p.message || "",
          tone: p.ok ? "ok" : "error",
        };
        atvvRepairing.value = false;
        await refreshHost();
      },
    );
  } catch (e) {
    console.warn("listen xiaomi-atvv-repair-result failed:", e);
  }

  try {
    unlistenAtvvCancel = await listen<{ message?: string }>(
      "xiaomi-atvv-repair-cancelled",
      (event) => {
        host.value = {
          ...host.value,
          status_text: "ATVV 修复已取消",
          detail: event.payload?.message || "已取消修复",
          tone: "warn",
        };
        atvvRepairing.value = false;
      },
    );
  } catch (e) {
    console.warn("listen xiaomi-atvv-repair-cancelled failed:", e);
  }

  try {
    unlistenWinuhidProgress = await listen<{
      downloaded: number;
      total?: number | null;
      percent?: number | null;
    }>("winuhid-download-progress", (event) => {
      if (!event.payload) return;
      winuhidDownloadPhase.value = "downloading";
      winuhidDownloadProgress.value = event.payload;
    });
  } catch (e) {
    console.warn("listen winuhid-download-progress failed:", e);
  }

  try {
    unlistenWinuhidComplete = await listen<{ path: string }>(
      "winuhid-download-complete",
      (event) => {
        winuhidDownloadPhase.value = "complete";
        const path = event.payload?.path || "";
        winuhidDownloadMessage.value = path
          ? `已保存到：${path}。请解压后阅读「安装说明.txt」，双击 Run-Install.cmd 安装。`
          : "下载完成。请解压后阅读「安装说明.txt」，双击 Run-Install.cmd 安装。";
        prependLog(winuhidDownloadMessage.value);
      },
    );
  } catch (e) {
    console.warn("listen winuhid-download-complete failed:", e);
  }

  try {
    unlistenWinuhidError = await listen<{ message: string }>(
      "winuhid-download-error",
      (event) => {
        winuhidDownloadPhase.value = "error";
        winuhidDownloadMessage.value = event.payload?.message || "下载失败";
        prependLog(`WinUHid 驱动包下载失败: ${winuhidDownloadMessage.value}`);
      },
    );
  } catch (e) {
    console.warn("listen winuhid-download-error failed:", e);
  }
});

onUnmounted(() => {
  unlistenKey?.();
  unlistenMeter?.();
  unlistenAtvvRepair?.();
  unlistenAtvvCancel?.();
  unlistenWinuhidProgress?.();
  unlistenWinuhidComplete?.();
  unlistenWinuhidError?.();
  if (hostPollTimer) clearInterval(hostPollTimer);
  if (devicePollTimer) clearInterval(devicePollTimer);
  if (voiceTipCloseTimer) clearTimeout(voiceTipCloseTimer);
  if (gainTipCloseTimer) clearTimeout(gainTipCloseTimer);
  if (triggerTipCloseTimer) clearTimeout(triggerTipCloseTimer);
  if (repairTipCloseTimer) clearTimeout(repairTipCloseTimer);
  if (winuhidTipCloseTimer) clearTimeout(winuhidTipCloseTimer);
  if (atvvTipCloseTimer) clearTimeout(atvvTipCloseTimer);
  if (restartTipCloseTimer) clearTimeout(restartTipCloseTimer);
  if (mappingFlashClearTimer) clearTimeout(mappingFlashClearTimer);
  if (tipViewportRaf != null) {
    cancelAnimationFrame(tipViewportRaf);
    tipViewportRaf = null;
  }
  window.removeEventListener("resize", onViewportChange);
  window.removeEventListener("scroll", onViewportChange, true);
});

watch(
  () => device.value.status,
  (status, prev) => {
    if (status === prev) return;
    if (status === "Connected") {
      const name = device.value.device_name || "MI RC";
      prependLog(`已连接 ${name}`);
    } else if (status === "Connecting") {
      prependLog("正在连接...");
    } else if (status === "Disconnected") {
      prependLog("已断开");
    } else if (status.startsWith("Error")) {
      prependLog(bridge.statusLabel(status));
    }
  }
);

function toggleConnection() {
  if (device.value.status === "Connected") {
    bridge.stopBridge(type);
  } else {
    bridge.startBridge(type);
  }
}

async function retryLoadConfig() {
  await configStore.loadConfig(type);
  if (configStore.loadErrors[type]) {
    prependLog(`配置加载失败: ${configStore.loadErrors[type]}`);
  }
}

</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="title-row">
        <h2>小米遥控器 2 Pro</h2>
      </div>
      <DeviceStatus
        :status="device.status"
        :loading="bridge.loading[type]"
        @toggle="toggleConnection"
      />
    </header>

    <div class="overview-row">
      <div class="overview-left">
        <div class="device-info-row">
          <div class="info-item" style="min-width: 140px !important;">
            <span class="info-label">设备名称</span>
            <span class="info-value">{{ device.device_name || "—" }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">蓝牙地址</span>
            <span class="info-value">{{ device.device_address || "—" }}</span>
          </div>
          <div class="info-item" >
            <span class="info-label">电量</span>
            <span class="info-value">
              {{ device.battery_level != null ? device.battery_level + "%" : "—" }}
            </span>
          </div>
          <div class="info-item">
  
            <span class="info-label">连接方式</span>
            <span class="info-value">蓝牙 BLE</span>
          </div>
          <div
            class="info-item info-item-audio"
            :class="{
              'is-session': voiceMeter.bleState === 'session',
              'is-receiving': voiceMeter.bleState === 'receiving',
            }"
            title="遥控器 BLE 解码后的 PCM"
          >
            <div class="audio-label-row">
              <span class="info-label">音频信号</span>
              <span
                v-if="showAtvvFailLabel"
                class="audio-atvv-fail"
              >ATVV 未连接</span>
              <span
                v-else-if="voiceMeter.bleState !== 'idle'"
                class="audio-state"
              >{{ bleSignalLabel }}</span>
            </div>
            <div class="ble-wave" aria-hidden="true">
              <span
                v-for="(v, i) in voiceMeter.waveform"
                :key="i"
                class="ble-wave-bar"
                :style="{ height: `${Math.max(8, Math.round(Math.pow(Math.min(1, Math.max(0, v)), 0.25) * 100))}%` }"
              />
            </div>
          </div>
        </div>

        <section class="card host-card">
          <div class="host-status-row" role="list" aria-label="运行状态">
            <div
              v-for="item in host.items"
              :key="item.id"
              class="host-status-item"
              :class="{ 'host-status-cable': item.id === 'cable' }"
              role="listitem"
            >
              <span
                class="host-dot"
                :class="itemToneClass(item.tone)"
                aria-hidden="true"
              />
              <span class="host-item-label">{{ item.label }}</span>
              <div
                v-if="item.id === 'cable'"
                class="cable-meter"
                :class="{ active: voiceMeter.cableActive }"
                :title="cableActivityLabel"
                aria-hidden="true"
              >
                <span class="cable-meter-track">
                  <span
                    class="cable-meter-fill"
                    :style="{ width: `${Math.round(voiceMeter.cableLevel * 100)}%` }"
                  />
                </span>
              </div>
              <span class="host-item-state" :class="itemToneClass(item.tone)">
                {{ item.state_label }}
              </span>
            </div>
          </div>
          <p v-if="host.detail" class="host-detail">{{ host.detail }}</p>
          <div class="host-actions">
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="voiceRepairing || restarting"
                @click="voiceDetectAndRepair"
              >
                {{ voiceRepairing ? "处理中..." : "虚拟声卡检测与修复" }}
              </button>
              <button
                ref="repairInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showRepairTip"
                aria-label="虚拟声卡检测与修复说明"
                @mouseenter="openRepairTip"
                @mouseleave="scheduleCloseRepairTip"
                @focus="openRepairTip"
                @blur="scheduleCloseRepairTip"
                @click.stop="toggleRepairTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showRepairTip"
                  ref="repairTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="repairTipStyle"
                  @mouseenter="openRepairTip"
                  @mouseleave="scheduleCloseRepairTip"
                >
                  <p class="tip-lead">
                    用来检查并修好电脑上的语音通路（VB-CABLE 虚拟声卡），让遥控器麦克风声音能进系统、供输入法听写。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>检测 VB-CABLE 是否已安装、是否可用</li>
                      <li>已装好则尝试自动修复配置</li>
                      <li>未安装时可选用内嵌驱动，或下载官网最新版</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>首次使用语音，或重装系统 / 换电脑后</li>
                      <li>按语音键没声音、输入法听不到遥控器</li>
                      <li>提示未检测到 VB-CABLE、语音环境异常时</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    平时语音正常就不必反复点；若提示必须重启电脑，按提示重启后再试。结果会写在右侧状态日志。
                  </p>
                </div>
              </Teleport>
            </div>
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="winuhidRepairing || voiceRepairing || atvvRepairing || restarting"
                @click="repairWinUHid"
              >
                {{ winuhidRepairing ? "修复中..." : "修复虚拟键盘" }}
              </button>
              <button
                ref="winuhidInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showWinuhidTip"
                aria-label="修复虚拟键盘说明"
                @mouseenter="openWinuhidTip"
                @mouseleave="scheduleCloseWinuhidTip"
                @focus="openWinuhidTip"
                @blur="scheduleCloseWinuhidTip"
                @click.stop="toggleWinuhidTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showWinuhidTip"
                  ref="winuhidTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="winuhidTipStyle"
                  @mouseenter="openWinuhidTip"
                  @mouseleave="scheduleCloseWinuhidTip"
                >
                  <p class="tip-lead">
                    在电脑里装一块「虚拟键盘」，让豆包、千问等输入法把遥控器的语音键当成真键盘按键，而不是普通模拟点击（那种方式常被输入法忽略）。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>部署 WinUHid 组件，并安装内嵌的虚拟键盘驱动</li>
                      <li>在系统里注册并启动虚拟键盘设备，让语音组合键按硬件方式注入</li>
                      <li>完成后状态栏「虚拟键盘」应显示就绪；仍不行可选导出安装包</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>首次用豆包 / 千问语音，或重装系统、换电脑后</li>
                      <li>状态里「虚拟键盘」未就绪，或按语音键唤不醒输入法</li>
                      <li>日志提示需要 WinUHid、语音键被拦截时</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    会弹出 UAC 管理员确认，请点允许。点按钮后会打开修复选项：自动修复、强制重装、导出到桌面或从 Release 下载。这和「虚拟声卡检测与修复」「修复 ATVV 连接」不是一回事。
                  </p>
                </div>
              </Teleport>
            </div>
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="atvvRepairing || restarting || voiceRepairing || winuhidRepairing"
                @click="repairAtvv"
              >
                {{ atvvRepairing ? "修复中..." : "修复 ATVV 连接" }}
              </button>
              <button
                ref="atvvInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showAtvvTip"
                aria-label="修复 ATVV 连接说明"
                @mouseenter="openAtvvTip"
                @mouseleave="scheduleCloseAtvvTip"
                @focus="openAtvvTip"
                @blur="scheduleCloseAtvvTip"
                @click.stop="toggleAtvvTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showAtvvTip"
                  ref="atvvTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="atvvTipStyle"
                  @mouseenter="openAtvvTip"
                  @mouseleave="scheduleCloseAtvvTip"
                >
                  <p class="tip-lead">
                    修好遥控器到电脑的「语音专用蓝牙通道」（ATVV）。通道正常后，按住语音键才有绿色音频波动，也不会误触发系统 F5 插入日期。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>检查是否有其它遥控桥接软件占用</li>
                      <li>暂停 HID Tap 后软重启连接，并重新订阅语音通道</li>
                      <li>有占用时会先弹窗让你结束相关进程，再继续修复</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>「音频信号」旁出现红字「ATVV 未连接」</li>
                      <li>按住语音键说话，绿色波形一直不动</li>
                      <li>按语音键后记事本等处插入了日期时间</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    平时语音和波形都正常就不必点。这和「虚拟声卡检测与修复」不同：那边管电脑声卡，这边管遥控器蓝牙语音通道。
                  </p>
                </div>
              </Teleport>
            </div>
            <div class="host-action-group">
              <button
                class="btn btn-secondary"
                type="button"
                :disabled="restarting || voiceRepairing || atvvRepairing"
                @click="restartBridge"
              >
                {{ restarting ? "重启中..." : "重启桥接" }}
              </button>
              <button
                ref="restartInfoBtn"
                type="button"
                class="title-info"
                :aria-expanded="showRestartTip"
                aria-label="重启桥接说明"
                @mouseenter="openRestartTip"
                @mouseleave="scheduleCloseRestartTip"
                @focus="openRestartTip"
                @blur="scheduleCloseRestartTip"
                @click.stop="toggleRestartTip"
              >
                <span class="title-info-icon" aria-hidden="true">i</span>
              </button>
              <Teleport to="body">
                <div
                  v-if="showRestartTip"
                  ref="restartTipEl"
                  class="floating-info-tip voice-info-tip"
                  role="tooltip"
                  :style="restartTipStyle"
                  @mouseenter="openRestartTip"
                  @mouseleave="scheduleCloseRestartTip"
                >
                  <p class="tip-lead">
                    软重启「与遥控器的蓝牙连接」，按最新配置重新连上；无需退出整个应用。
                  </p>
                  <div class="tip-block tip-on">
                    <div class="tip-badge">会做什么</div>
                    <ul>
                      <li>停止并重新拉起蓝牙 / ATVV 连接</li>
                      <li>按当前映射、增益等配置重新尝试连接遥控器</li>
                      <li>语音路由异常时也会顺带尝试拉起</li>
                    </ul>
                  </div>
                  <div class="tip-block tip-off">
                    <div class="tip-badge">什么时候点</div>
                    <ul>
                      <li>改了增益、映射等设置后不生效</li>
                      <li>状态显示异常、按键失灵、连上又掉线</li>
                      <li>长时间不用后突然不响应，想快速恢复</li>
                    </ul>
                  </div>
                  <p class="tip-foot">
                    返回 / 音量专用通道会尽量保持，一般不必为此反复重启。若仍无效，可再试「虚拟声卡检测与修复」，或查看日志。
                  </p>
                </div>
              </Teleport>
            </div>
            <button class="btn btn-secondary" type="button" @click="openLogs">
              日志
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              @click="showSetupTips = true"
            >
              输入法设置
            </button>
          </div>
        </section>
      </div>

      <aside class="log-aside">
        <section class="card log-card">
          <p class="card-text">状态日志</p>
          <div ref="logAreaRef" class="log-area">
            <p v-for="entry in logs" :key="entry.id" class="log-entry">
              <span class="log-time">{{ entry.time }}</span>
              <span class="log-text">{{ entry.text }}</span>
            </p>
          </div>
        </section>
      </aside>
    </div>

    <div class="page-body">
      <!-- 小米专用运行状态弹层等 -->
      <div v-if="showSetupTips" class="voice-modal-backdrop" @click.self="showSetupTips = false">
        <div class="voice-modal setup-tips-modal" role="dialog" aria-modal="true" aria-labelledby="setup-tips-title">
          <div class="setup-tips-head">
            <h3 id="setup-tips-title">输入法设置</h3>
            <button class="btn btn-secondary" type="button" @click="showSetupTips = false">关闭</button>
          </div>
          <p class="setup-tips-lead">
            语音键快捷键要和输入法里设的一样，可先点「快速应用」。
          </p>
          <p v-if="setupApplyHint" class="setup-apply-hint setup-apply-hint-global">
            {{ setupApplyHint }}
          </p>

          <div class="setup-ime-tabs" role="tablist" aria-label="输入法分类">
            <button
              v-for="tab in imeTabs"
              :key="tab.id"
              type="button"
              class="setup-ime-tab"
              :class="{ active: setupImeTab === tab.id }"
              role="tab"
              :aria-selected="setupImeTab === tab.id"
              @click="setupImeTab = tab.id"
            >
              {{ tab.label }}
            </button>
          </div>

          <div v-if="setupImeTab === 'faq'" class="setup-ime-panel" role="tabpanel">
            <div class="setup-ime-warn setup-ime-faq-warn" role="note">
              <p class="setup-ime-warn-title">{{ imeFaq.warnTitle }}</p>
            </div>
            <section
              v-for="(section, sIdx) in imeFaq.sections"
              :key="sIdx"
              class="setup-faq-section"
            >
              <h4 class="setup-faq-section-title">{{ section.title }}</h4>
              <ol class="setup-ime-steps setup-faq-list">
                <li v-for="(item, idx) in section.items" :key="idx">{{ item }}</li>
              </ol>
            </section>
          </div>

          <div v-else-if="setupImeTab === 'qianwen'" class="setup-ime-panel" role="tabpanel">
            <article class="setup-ime-card">
              <header class="setup-ime-head">
                <h4>{{ qianwenGuide.title }}</h4>
                <span class="setup-ime-tag">{{ qianwenGuide.tag }}</span>
              </header>
              <ol class="setup-ime-steps">
                <li v-for="(step, idx) in qianwenGuide.steps" :key="idx">{{ step }}</li>
              </ol>
              <div class="setup-ime-apply setup-ime-apply-row">
                <button
                  v-for="preset in qianwenPresets"
                  :key="preset.id"
                  class="btn btn-primary"
                  type="button"
                  :disabled="!config"
                  @click="applyImePreset(preset.id)"
                >
                  快速应用：{{ presetShortcutLabel(preset) }}
                </button>
              </div>
            </article>
          </div>

          <div v-else class="setup-ime-panel" role="tabpanel">
            <article
              v-for="preset in activeImePresets"
              :key="preset.id"
              class="setup-ime-card"
            >
              <header class="setup-ime-head">
                <h4>{{ preset.title }}</h4>
                <span class="setup-ime-tag">{{ preset.tag }}</span>
              </header>
              <ol class="setup-ime-steps">
                <li v-for="(step, idx) in preset.steps" :key="idx">{{ step }}</li>
              </ol>
              <div class="setup-ime-apply">
                <button
                  class="btn btn-primary"
                  type="button"
                  :disabled="!config"
                  @click="applyImePreset(preset.id)"
                >
                  快速应用：{{ presetShortcutLabel(preset) }}
                </button>
              </div>
              <figure v-if="isWechatPreset(preset.id)" class="setup-ime-figure">
                <figcaption>微信 · 「按住说话」快捷键</figcaption>
                <img
                  :src="wechatImeHotkeysImg"
                  alt="微信输入法按住说话快捷键设置"
                  class="setup-ime-img"
                />
              </figure>
              <figure v-if="isDoubaoHoldPreset(preset.id)" class="setup-ime-figure">
                <figcaption>豆包 · 「长按模式」快捷键</figcaption>
                <img
                  :src="doubaoImeHotkeysImg"
                  alt="豆包输入法长按模式快捷键设置"
                  class="setup-ime-img"
                />
              </figure>
            </article>
          </div>
        </div>
      </div>

      <div v-if="showLogModal" class="voice-modal-backdrop" @click.self="showLogModal = false">
        <div class="voice-modal log-modal" role="dialog" aria-modal="true">
          <h3>运行日志</h3>
          <p v-if="logPath" class="log-path">{{ logPath }}</p>
          <pre class="log-viewer">{{ logLoading ? "读取中…" : logText }}</pre>
          <div class="log-modal-actions">
            <button class="btn btn-primary" type="button" :disabled="logLoading" @click="copyLog">
              {{ logCopyHint || "复制" }}
            </button>
            <button class="btn btn-secondary" type="button" @click="openLogExternally">
              用记事本打开
            </button>
            <button class="btn btn-secondary" type="button" @click="showLogModal = false">
              关闭
            </button>
          </div>
        </div>
      </div>
      <div v-if="showVoiceChoice" class="voice-modal-backdrop" @click.self="showVoiceChoice = false">
        <div class="voice-modal" role="dialog" aria-modal="true">
          <h3>未检测到 VB-CABLE</h3>
          <p>{{ voiceChoiceMsg || "请选择安装方式：" }}</p>
          <p class="voice-modal-uac-tip">如弹出 Windows 管理员确认（UAC），点同意</p>
          <p class="voice-modal-reboot-tip">安装完成必须重启系统</p>
          <div class="voice-modal-actions">
            <button
              class="btn btn-primary"
              type="button"
              :disabled="voiceRepairing"
              @click="chooseVoiceSource('embedded')"
            >
              使用内嵌驱动安装
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="voiceRepairing"
              @click="chooseVoiceSource('download_zip')"
            >
              下载最新驱动包手动安装
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="voiceRepairing"
              @click="chooseVoiceSource('download_page')"
            >
              打开VB-CABLE官网
            </button>
            <button class="btn btn-secondary" type="button" @click="showVoiceChoice = false">
              取消
            </button>
          </div>
          <p class="voice-modal-note">
            内嵌为已校验的 VB-CABLE 4.5；安装时会弹出 Windows 管理员确认。官网下载适合需要更新版本时使用。
          </p>
        </div>
      </div>

      <div
        v-if="showVoiceReboot"
        class="voice-modal-backdrop"
        @click.self="showVoiceReboot = false"
      >
        <div
          class="voice-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="voice-reboot-title"
        >
          <h3 id="voice-reboot-title">需要重启 Windows</h3>
          <p>{{ voiceRebootMsg || "驱动已安装，必须重启系统后虚拟声卡才会生效。" }}</p>
          <p class="voice-modal-reboot-tip">安装完成必须重启系统</p>
          <div class="voice-modal-actions">
            <button class="btn btn-primary" type="button" @click="showVoiceReboot = false">
              知道了
            </button>
          </div>
        </div>
      </div>

      <div
        v-if="showWinuhidChoice"
        class="voice-modal-backdrop"
        @click.self="winuhidDownloadPhase !== 'downloading' && (showWinuhidChoice = false)"
      >
        <div class="voice-modal" role="dialog" aria-modal="true">
          <h3>虚拟键盘修复</h3>
          <p>{{ winuhidChoiceMsg || "请选择修复或安装方式：" }}</p>
          <p class="voice-modal-uac-tip">自动修复会弹出 UAC；导出包请阅读「安装说明.txt」后双击 Run-Install.cmd</p>
          <p class="voice-modal-reboot-tip">部分电脑安装后必须重启 Windows</p>

          <div
            v-if="winuhidDownloadPhase !== 'idle'"
            class="winuhid-download-progress"
            role="status"
            aria-live="polite"
          >
            <div class="winuhid-download-head">
              <span class="winuhid-download-label">
                {{
                  winuhidDownloadPhase === "downloading"
                    ? "正在下载驱动包…"
                    : winuhidDownloadPhase === "complete"
                      ? "下载完成"
                      : "下载失败"
                }}
              </span>
              <span
                v-if="winuhidDownloadPhase === 'downloading'"
                class="winuhid-download-meta"
              >
                {{ winuhidDownloadProgressLabel }}
              </span>
            </div>
            <div
              class="winuhid-download-track"
              :class="{
                indeterminate:
                  winuhidDownloadPhase === 'downloading' &&
                  winuhidDownloadProgress?.percent == null,
              }"
            >
              <div
                class="winuhid-download-bar"
                :style="{ width: winuhidDownloadProgressWidth() }"
              />
            </div>
            <p v-if="winuhidDownloadMessage" class="winuhid-download-msg">
              {{ winuhidDownloadMessage }}
            </p>
          </div>

          <div class="voice-modal-actions">
            <button
              class="btn btn-primary"
              type="button"
              :disabled="winuhidRepairing || winuhidDownloadPhase === 'downloading'"
              @click="chooseWinuhidSource('embedded')"
            >
              自动修复
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="winuhidRepairing || winuhidDownloadPhase === 'downloading'"
              @click="chooseWinuhidSource('embedded_force')"
            >
              强制重装（完整走一遍安装）
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="winuhidRepairing || winuhidDownloadPhase === 'downloading'"
              @click="chooseWinuhidSource('export')"
            >
              导出到桌面手动安装
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="winuhidRepairing || winuhidDownloadPhase === 'downloading'"
              @click="chooseWinuhidSource('download_zip')"
            >
              {{
                winuhidDownloadPhase === "downloading"
                  ? "下载中…"
                  : "下载驱动包手动安装"
              }}
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="winuhidRepairing || winuhidDownloadPhase === 'downloading'"
              @click="chooseWinuhidSource('download_page')"
            >
              打开 Release 页
            </button>
            <button
              class="btn btn-secondary"
              type="button"
              :disabled="winuhidDownloadPhase === 'downloading'"
              @click="showWinuhidChoice = false"
            >
              取消
            </button>
          </div>
          <p class="voice-modal-note">
            导出/下载包内含「安装说明.txt」与 Run-Install.cmd。虚拟键盘已就绪时「自动修复」会跳过；需完整验证请选「强制重装」。
          </p>
        </div>
      </div>

      <section v-if="configSectionLoading" class="card mapping-layout mapping-placeholder">
        <h3>按键映射</h3>
        <p class="mapping-placeholder-text">正在加载按键映射…</p>
      </section>

      <section v-else-if="configLoadState === 'error'" class="card mapping-layout mapping-error">
        <h3>按键映射</h3>
        <p class="mapping-error-text">
          配置未能加载，按键映射区域无法显示。
          <span v-if="configLoadError">（{{ configLoadError }}）</span>
        </p>
        <button class="btn btn-secondary" type="button" @click="retryLoadConfig">
          重试加载
        </button>
      </section>

      <section v-else-if="config" class="card mapping-layout">
        <div class="mapping-heading">
          <h3>按键映射</h3>
          <p
            v-if="lastMappingFlash"
            :key="lastMappingFlash.seq"
            class="mapping-flash"
            role="status"
            aria-live="polite"
          >
            <span class="mapping-flash-phase">{{
              lastMappingFlash.phase === "up" ? "抬起" : "按下"
            }}</span>
            <span class="mapping-flash-remote">{{ lastMappingFlash.remote }}</span>
            <template v-if="lastMappingFlash.mapped">
              <span class="mapping-flash-sep" aria-hidden="true">：</span>
              <span class="mapping-flash-mapped">{{ lastMappingFlash.mapped }}</span>
            </template>
          </p>
        </div>
        <div class="voice-toolbar" role="group" aria-label="语音听写设置">
          <div class="voice-toolbar-item">
            <span class="voice-toolbar-label">点击语音键是否发送映射按键</span>
            <label class="switch" title="点击语音键是否发送映射按键">
              <input
                type="checkbox"
                v-model="voiceShortcutEnabled"
                aria-label="点击语音键是否发送映射按键"
              />
              <span class="switch-slider" aria-hidden="true"></span>
            </label>
            <button
              ref="voiceInfoBtn"
              type="button"
              class="title-info voice-info"
              :aria-expanded="showVoiceShortcutTip"
              aria-label="语音映射按键说明"
              @mouseenter="openVoiceTip"
              @mouseleave="scheduleCloseVoiceTip"
              @focus="openVoiceTip"
              @blur="scheduleCloseVoiceTip"
              @click.stop="toggleVoiceTip"
            >
              <span class="title-info-icon" aria-hidden="true">i</span>
            </button>
            <Teleport to="body">
              <div
                v-if="showVoiceShortcutTip"
                ref="voiceTipEl"
                class="floating-info-tip voice-info-tip"
                role="tooltip"
                :style="voiceTipStyle"
                @mouseenter="openVoiceTip"
                @mouseleave="scheduleCloseVoiceTip"
              >
                <p class="tip-lead">
                  只管「按语音键时要不要发映射快捷键」。传声（VB-CABLE）不受此开关影响。
                </p>
                <div class="tip-block tip-on">
                  <div class="tip-badge">开</div>
                  <ul>
                    <li>声音送到电脑</li>
                    <li>按住语音键时发送你设好的映射快捷键</li>
                  </ul>
                  <p class="tip-aside">适合靠快捷键开/关的语音输入法。</p>
                </div>
                <div class="tip-block tip-off">
                  <div class="tip-badge">关</div>
                  <ul>
                    <li>声音照样送到电脑</li>
                    <li>不发送映射键（日志只记按下/抬起语音键）</li>
                  </ul>
                  <p class="tip-aside">听写需自行打开输入法语音。</p>
                </div>
              </div>
            </Teleport>
          </div>

          <div v-if="SHOW_VOICE_TRIGGER_MODE" class="voice-toolbar-item">
            <span class="voice-toolbar-label">触发模式</span>
            <select
              v-model="config.trigger_mode"
              class="form-select voice-toolbar-select"
              @change="persistVoiceSettings"
            >
              <option value="Toggle">点击</option>
              <option value="Hold">按住</option>
            </select>
            <button
              ref="triggerInfoBtn"
              type="button"
              class="title-info voice-info"
              :aria-expanded="showTriggerTip"
              aria-label="触发模式说明"
              @mouseenter="openTriggerTip"
              @mouseleave="scheduleCloseTriggerTip"
              @focus="openTriggerTip"
              @blur="scheduleCloseTriggerTip"
              @click.stop="toggleTriggerTip"
            >
              <span class="title-info-icon" aria-hidden="true">i</span>
            </button>
            <Teleport to="body">
              <div
                v-if="showTriggerTip"
                ref="triggerTipEl"
                class="floating-info-tip voice-info-tip"
                role="tooltip"
                :style="triggerTipStyle"
                @mouseenter="openTriggerTip"
                @mouseleave="scheduleCloseTriggerTip"
              >
                <p class="tip-lead">
                  快捷键跟随遥控器实际操作：点一下就点按，按住就按住。
                </p>
                <div class="tip-block tip-on">
                  <div class="tip-badge">点击</div>
                  <ul>
                    <li>短按语音键：点按一次映射快捷键</li>
                    <li>长按语音键：按住映射快捷键，松手释放</li>
                  </ul>
                  <p class="tip-aside">适合「点一下开/关」类输入法，也会正确处理长按。</p>
                </div>
                <div class="tip-block tip-off">
                  <div class="tip-badge">按住</div>
                  <ul>
                    <li>按下语音键：立刻按住映射快捷键并传声</li>
                    <li>松开语音键：释放快捷键并结束</li>
                  </ul>
                  <p class="tip-aside">适合「按住说话」类输入法。</p>
                </div>
              </div>
            </Teleport>
          </div>

          <div class="voice-toolbar-item">
            <span class="voice-toolbar-label">增益 (dB)</span>
            <div class="number-stepper" role="group" aria-label="增益分贝">
              <button
                type="button"
                class="stepper-btn"
                aria-label="减小增益"
                :disabled="gainDb <= GAIN_MIN"
                @click="stepGain(-GAIN_STEP)"
              >
                −
              </button>
              <input
                type="number"
                class="gain-input"
                v-model.number="gainDb"
                :min="GAIN_MIN"
                :max="GAIN_MAX"
                :step="GAIN_STEP"
                @blur="clampGainOnBlur"
              />
              <button
                type="button"
                class="stepper-btn"
                aria-label="增大增益"
                :disabled="gainDb >= GAIN_MAX"
                @click="stepGain(GAIN_STEP)"
              >
                +
              </button>
            </div>
            <button
              ref="gainInfoBtn"
              type="button"
              class="title-info voice-info"
              :aria-expanded="showGainTip"
              aria-label="增益说明"
              @mouseenter="openGainTip"
              @mouseleave="scheduleCloseGainTip"
              @focus="openGainTip"
              @blur="scheduleCloseGainTip"
              @click.stop="toggleGainTip"
            >
              <span class="title-info-icon" aria-hidden="true">i</span>
            </button>
            <Teleport to="body">
              <div
                v-if="showGainTip"
                ref="gainTipEl"
                class="floating-info-tip voice-info-tip"
                role="tooltip"
                :style="gainTipStyle"
                @mouseenter="openGainTip"
                @mouseleave="scheduleCloseGainTip"
              >
                <p class="tip-lead">
                  增益 = 把遥控器麦克风声音「放大或缩小」再送进电脑（VB-CABLE）。
                  只影响音量大小，不改变能不能说话。
                </p>
                <div class="tip-block tip-on">
                  <div class="tip-badge">怎么调</div>
                  <ul>
                    <li>听不清、识别漏字 → 调高（如 10 → 14）</li>
                    <li>破音、刺耳、识别乱 → 调低（如 10 → 6）</li>
                    <li>常用默认 <strong>10 dB</strong>；范围 -12 ～ 30</li>
                  </ul>
                </div>
                <div class="tip-block tip-off">
                  <div class="tip-badge">注意</div>
                  <ul>
                    <li>改完后请重新连接遥控器，或点「重启桥接」后生效</li>
                    <li>一次加减 2～4 dB 即可，别一次拉满</li>
                  </ul>
                </div>
                <p class="tip-foot">
                  简单记：声音太小就加，太吵就减。
                </p>
              </div>
            </Teleport>
          </div>
        </div>
        <KeyMappingStage
          :config="config"
          @save="(cfg) => configStore.saveConfig(type, cfg)"
        />
      </section>
    </div>
  </div>
</template>

<style scoped>
.page {
  width: 100%;
  max-width: none;
  box-sizing: border-box;
}
.mapping-heading {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 6px 16px;
  margin-bottom: 8px;
  min-height: 1.4em;
}
.mapping-layout h3 {
  margin: 0;
  flex: 0 0 auto;
}
.mapping-placeholder,
.mapping-error {
  min-height: 160px;
}
.mapping-placeholder-text,
.mapping-error-text {
  margin: 10px 0 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-secondary);
}
.mapping-error-text {
  margin-bottom: 12px;
}
.mapping-flash {
  margin: 0;
  padding: 0;
  font-size: 13px;
  line-height: 1.35;
  color: var(--text-muted, #64748b);
  animation: mapping-flash-in 0.28s ease-out;
}
.mapping-flash-phase {
  margin-right: 6px;
  color: var(--text-muted, #94a3b8);
  font-weight: 500;
}
.mapping-flash-remote {
  color: var(--text, #334155);
  font-weight: 600;
}
.mapping-flash-sep {
  margin: 0 1px;
  color: var(--text-muted, #94a3b8);
}
.mapping-flash-mapped {
  color: var(--accent, #0f766e);
  font-weight: 600;
}
@keyframes mapping-flash-in {
  from {
    opacity: 0;
    transform: translateX(-4px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.voice-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: stretch;
  gap: 10px;
  margin-bottom: 12px;
  padding: 0;
  border: none;
  background: transparent;
}
.voice-toolbar-item {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 8px;
  flex: 1 1 auto;
  min-width: max-content;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #fff;
}
.voice-toolbar-label {
  font-size: 13px;
  line-height: 1.3;
  font-weight: 500;
  color: var(--text);
  white-space: nowrap;
}
.voice-toolbar-select {
  min-width: 72px;
  padding: 4px 8px;
}
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.title-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.page-header h2 { font-size: 20px; font-weight: 600; margin: 0; }
.title-info {
  position: relative;
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  padding: 0;
  border: 1.5px solid #94a3b8;
  border-radius: 50%;
  background: transparent;
  color: #64748b;
  cursor: help;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.title-info:hover,
.title-info:focus-visible {
  border-color: #2563eb;
  color: #2563eb;
  outline: none;
}
.title-info-icon {
  font-size: 11px;
  font-weight: 700;
  font-style: italic;
  font-family: Georgia, "Times New Roman", serif;
  line-height: 1;
}

.switch {
  position: relative;
  display: inline-block;
  width: 40px;
  height: 22px;
  flex-shrink: 0;
}
.switch input {
  opacity: 0;
  width: 0;
  height: 0;
  position: absolute;
}
.switch-slider {
  position: absolute;
  inset: 0;
  border-radius: 999px;
  background: #cbd5e1;
  cursor: pointer;
  transition: background 0.15s ease;
}
.switch-slider::before {
  content: "";
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.2);
  transition: transform 0.15s ease;
}
.switch input:checked + .switch-slider {
  background: var(--primary, #2563eb);
}
.switch input:checked + .switch-slider::before {
  transform: translateX(18px);
}
.switch input:focus-visible + .switch-slider {
  outline: 2px solid rgba(37, 99, 235, 0.35);
  outline-offset: 2px;
}

.device-info-row {
  display: grid;
  grid-template-columns: 1fr 1fr 0.75fr 0.85fr minmax(140px, 1.55fr);
  gap: 10px 16px;
  margin-bottom: 0;
  padding: 12px 14px;
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  align-items: start;
}
@media (max-width: 720px) {
  .device-info-row {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .info-item-audio {
    grid-column: 1 / -1;
  }
}

.overview-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  gap: 12px;
  align-items: stretch;
  margin-bottom: 16px;
}
.overview-left {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
/* 仅由左侧撑高；日志绝对铺满同高 */
.log-aside {
  position: relative;
  min-height: 0;
}
.log-card {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  min-width: 0;
  width: auto;
  max-width: none;
  padding: 5px;
  overflow: hidden;
  box-sizing: border-box;
}
.log-card h3 {
  margin: 0 0 6px;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 400;
  color: var(--text-secondary);
}
@media (max-width: 840px) {
  .overview-row {
    grid-template-columns: 1fr;
  }
  .log-aside {
    position: static;
    height: 180px;
  }
  .log-card {
    position: relative;
    inset: auto;
    height: 100%;
  }
}

.page-body { display: flex; flex-direction: column; gap: 16px; }

.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px;
}

.card-text {
  font-size: 12px;
  
  margin-bottom: 8px;
  color: var(--text);
}

.host-card {
  padding: 16px 18px;
}
.host-status-row {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
  align-items: stretch;
  margin: 0 0 12px;
}
.host-status-item {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  padding: 10px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #fff;
}
.host-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  background: #94a3b8;
}
.host-dot.ok {
  background: var(--success, #22c55e);
}
.host-dot.warn {
  background: var(--warning, #f59e0b);
}
.host-dot.error {
  background: var(--danger, #ef4444);
}
.host-item-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
  white-space: nowrap;
  flex-shrink: 0;
}
.host-item-state {
  margin-left: auto;
  padding-left: 4px;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  white-space: nowrap;
}
.host-item-state.ok {
  color: #15803d;
}
.host-item-state.warn {
  color: #b45309;
}
.host-item-state.error {
  color: #b91c1c;
}
.host-status-cable {
  flex-wrap: nowrap;
}
@media (min-width: 841px) and (max-width: 980px) {
  /* 侧栏日志并排时内容区偏窄，改为 2×2 避免块内文字挤出 */
  .host-status-row {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
.cable-meter {
  flex: 0 1 25%;
  max-width: 25%;
  min-width: 12px;
  display: flex;
  align-items: center;
}
.cable-meter-track {
  flex: 1;
  height: 6px;
  border-radius: 3px;
  background: #e2e8f0;
  overflow: hidden;
}
.cable-meter-fill {
  display: block;
  height: 100%;
  width: 0;
  border-radius: 3px;
  background: #94a3b8;
  transition: width 70ms linear;
}
.cable-meter.active .cable-meter-fill {
  background: #16a34a;
}
.host-detail {
  margin: 0 0 14px;
  font-size: 13px;
  color: #555;
  line-height: 1.5;
}
.host-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}
.host-action-group {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.btn {
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s ease;
}
.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.btn-secondary {
  background: #f1f5f9;
  color: var(--text);
  border: 1px solid var(--border);
}
.btn-secondary:hover:not(:disabled) {
  background: #e2e8f0;
}
.btn-primary {
  background: var(--primary, #2563eb);
  color: #fff;
  border: 1px solid transparent;
}
.btn-primary:hover:not(:disabled) {
  filter: brightness(0.95);
}

.voice-modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1000;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
}
.voice-modal {
  width: min(440px, 100%);
  background: var(--card-bg, #fff);
  border: 1px solid var(--border);
  border-radius: var(--radius, 8px);
  padding: 20px 22px;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.18);
}
.voice-modal h3 {
  margin: 0 0 10px;
  font-size: 16px;
}
.voice-modal p {
  margin: 0 0 16px;
  font-size: 13px;
  color: #555;
  line-height: 1.5;
}
.voice-modal-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.winuhid-download-progress {
  margin-bottom: 12px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border);
}
.winuhid-download-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 6px;
}
.winuhid-download-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}
.winuhid-download-meta {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
}
.winuhid-download-track {
  height: 8px;
  border-radius: 999px;
  background: #e2e8f0;
  overflow: hidden;
}
.winuhid-download-track.indeterminate .winuhid-download-bar {
  width: 35% !important;
  animation: winuhid-progress-indeterminate 1.2s ease-in-out infinite;
}
.winuhid-download-bar {
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #3b82f6, #1d4ed8);
  transition: width 0.15s ease;
}
.winuhid-download-msg {
  margin: 8px 0 0;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-secondary);
  word-break: break-all;
}
@keyframes winuhid-progress-indeterminate {
  0% {
    transform: translateX(-120%);
  }
  100% {
    transform: translateX(320%);
  }
}
.voice-modal-uac-tip {
  margin: -8px 0 8px !important;
  font-size: 13px !important;
  font-weight: 600;
  color: #ea580c !important;
  line-height: 1.45;
}
.voice-modal-reboot-tip {
  margin: 0 0 16px !important;
  font-size: 14px !important;
  font-weight: 700;
  color: #dc2626 !important;
  text-align: center;
  line-height: 1.45;
}
.voice-modal-note {
  margin-top: 14px !important;
  margin-bottom: 0 !important;
  font-size: 12px !important;
  color: #777 !important;
}

.log-modal {
  width: min(720px, 100%);
  max-height: min(80vh, 720px);
  display: flex;
  flex-direction: column;
}

.setup-tips-modal {
  width: min(560px, 100%);
  max-height: min(86vh, 820px);
  overflow-x: hidden;
  overflow-y: auto;
  padding: 16px 18px 18px;
  overscroll-behavior: contain;
  scrollbar-gutter: stable;
}
.setup-tips-modal::-webkit-scrollbar {
  width: 8px;
}
.setup-tips-modal::-webkit-scrollbar-track {
  background: #f1f5f9;
  border-radius: 4px;
}
.setup-tips-modal::-webkit-scrollbar-thumb {
  background: #94a3b8;
  border-radius: 4px;
}
.setup-tips-modal::-webkit-scrollbar-thumb:hover {
  background: #64748b;
}
.setup-tips-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
  position: sticky;
  top: -16px;
  z-index: 1;
  margin-left: -2px;
  margin-right: -2px;
  padding: 2px;
  background: var(--card-bg, #fff);
}
.setup-tips-head h3 {
  margin: 0;
}
.setup-tips-head .btn {
  padding: 4px 10px;
  font-size: 12px;
}
.setup-tips-lead {
  margin: 0 0 10px !important;
  font-size: 12px !important;
  color: #64748b !important;
}
.setup-ime-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin: 0 0 14px;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 28px;
  z-index: 1;
  background: var(--card-bg, #fff);
}
.setup-ime-tab {
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #f8fafc;
  color: #475569;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition:
    background 0.15s,
    border-color 0.15s,
    color 0.15s;
}
.setup-ime-tab:hover {
  background: #f1f5f9;
  border-color: #cbd5e1;
}
.setup-ime-tab.active {
  background: #eff6ff;
  border-color: #93c5fd;
  color: #1d4ed8;
}
.setup-ime-panel {
  min-height: 120px;
}
.setup-faq-section + .setup-faq-section {
  margin-top: 14px;
}
.setup-faq-section-title {
  margin: 0 0 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
.setup-faq-list {
  margin-bottom: 0 !important;
}
.setup-ime-faq-warn {
  margin-bottom: 14px;
}
.setup-ime-card {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px 14px 14px;
  background: #fafbfc;
}
.setup-ime-card + .setup-ime-card {
  margin-top: 12px;
}
.setup-ime-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.setup-ime-head h4 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
}
.setup-ime-tag {
  flex-shrink: 0;
  font-size: 11px;
  color: #2563eb;
  background: #eff6ff;
  border: 1px solid #bfdbfe;
  border-radius: 4px;
  padding: 2px 6px;
}
.setup-ime-warn {
  margin: 0 0 12px;
  padding: 10px 12px;
  border: 1px solid #fecaca;
  border-radius: 6px;
  background: #fef2f2;
}
.setup-ime-warn-title {
  margin: 0 0 8px !important;
  font-size: 13px !important;
  font-weight: 600 !important;
  color: #dc2626 !important;
  line-height: 1.5 !important;
}
.setup-ime-warn-sub {
  margin: 0 0 4px !important;
  font-size: 12px !important;
  font-weight: 600 !important;
  color: #b91c1c !important;
}
.setup-ime-warn-ways {
  margin: 0;
  padding-left: 1.25em;
  font-size: 12px;
  line-height: 1.55;
  color: #b91c1c;
}
.setup-ime-warn-ways li + li {
  margin-top: 4px;
}
.setup-ime-steps {
  margin: 0 0 12px;
  padding-left: 1.25em;
  font-size: 13px;
  line-height: 1.55;
  color: #334155;
}
.setup-ime-steps li + li {
  margin-top: 4px;
}
.setup-ime-steps code {
  font-size: 12px;
  padding: 1px 5px;
  border-radius: 3px;
  background: #e2e8f0;
  color: #0f172a;
}
.setup-ime-apply {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}
.setup-ime-apply .btn {
  padding: 6px 12px;
  font-size: 13px;
}
.setup-ime-apply-row {
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
  margin-bottom: 0;
}
.setup-ime-apply-row .btn {
  width: 100%;
  text-align: center;
  justify-content: center;
}
.setup-apply-hint {
  font-size: 12px;
  color: #16a34a;
}
.setup-apply-hint-global {
  margin: 0 0 10px;
}
.setup-ime-figure {
  margin: 0;
}
.setup-ime-img {
  display: block;
  width: 100%;
  max-width: 100%;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: #fff;
}
.setup-ime-figure figcaption {
  margin-top: 6px;
  font-size: 11px;
  color: #94a3b8;
  text-align: center;
}
.log-path {
  margin: 0 0 8px !important;
  font-size: 11px !important;
  color: #888 !important;
  word-break: break-all;
}
.log-viewer {
  flex: 1;
  min-height: 240px;
  max-height: 48vh;
  margin: 0 0 14px;
  padding: 10px 12px;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #0f172a;
  color: #e2e8f0;
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, Consolas, "Courier New", monospace;
}
.log-modal-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.info-item-audio {
  gap: 3px;
}
.audio-label-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}
.audio-label-row .info-label {
  flex-shrink: 0;
}
.audio-state {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  line-height: 1.2;
  white-space: nowrap;
}
.audio-atvv-fail {
  font-size: 12px;
  font-weight: 600;
  color: var(--danger, #ef4444);
  line-height: 1.2;
  white-space: nowrap;
}
.info-item-audio.is-session .audio-state {
  color: #b45309;
}
.info-item-audio.is-receiving .audio-state {
  color: #15803d;
}
.ble-wave {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 28px;
  padding: 3px 4px;
  border-radius: 4px;
  background: #f1f5f9;
  border: 1px solid var(--border);
}
.info-item-audio.is-receiving .ble-wave {
  background: #ecfdf5;
  border-color: #bbf7d0;
}
.info-item-audio.is-session .ble-wave {
  background: #fffbeb;
  border-color: #fde68a;
}
.ble-wave-bar {
  flex: 1 1 0;
  min-width: 2px;
  max-width: 6px;
  height: 8%;
  border-radius: 1px;
  background: #94a3b8;
  transition: height 60ms linear;
}
.info-item-audio.is-receiving .ble-wave-bar {
  background: #16a34a;
  /* 有输入时柱子的最小占用高度：远距离小信号也保持可见波动 */
  min-height: 20%;
}
.info-item-audio.is-session .ble-wave-bar {
  background: #d97706;
}

.info-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.info-value {
  font-size: 14px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.form-select {
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 4px;
  font-size: 13px;
  background: var(--card-bg);
}

.number-stepper {
  display: inline-flex;
  align-items: stretch;
  border: 1px solid var(--border);
  border-radius: 4px;
  overflow: hidden;
  background: var(--card-bg);
}

.stepper-btn {
  width: 30px;
  padding: 0;
  border: none;
  background: #f1f5f9;
  color: var(--text);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  user-select: none;
}

.stepper-btn:hover:not(:disabled) {
  background: #e2e8f0;
}

.stepper-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.stepper-btn + .gain-input,
.gain-input + .stepper-btn {
  border-left: 1px solid var(--border);
}

.gain-input {
  width: 56px;
  padding: 6px 4px;
  border: none;
  border-radius: 0;
  font-size: 13px;
  text-align: center;
  background: transparent;
  font-variant-numeric: tabular-nums;
  -moz-appearance: textfield;
  appearance: textfield;
}

.gain-input::-webkit-outer-spin-button,
.gain-input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.gain-input:focus {
  outline: none;
  background: #f8fafc;
}

.log-area {
  background: #f1f5f9;
  border-radius: 4px;
  padding: 6px 10px;
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.45;
}

.log-entry {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  color: var(--text);
  margin: 0 0 4px;
  white-space: normal;
}

.log-time {
  color: var(--text-secondary);
  flex-shrink: 0;
}

.log-text {
  min-width: 0;
  flex: 1;
  overflow-wrap: anywhere;
  word-break: break-word;
  white-space: pre-wrap;
}
</style>

<style>
/* Teleport 到 body：不用 scoped，避免样式丢失 */
.floating-info-tip {
  box-sizing: border-box;
  width: min(420px, calc(100vw - 16px));
  padding: 10px 12px;
  border-radius: 8px;
  background: #0f172a;
  color: #f8fafc;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.55;
  text-align: left;
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.28);
  white-space: normal;
  pointer-events: auto;
}
.floating-info-tip.voice-info-tip {
  width: min(360px, calc(100vw - 16px));
  padding: 12px 14px;
}
.floating-info-tip .tip-lead {
  margin: 0 0 10px;
  color: #e2e8f0;
  line-height: 1.55;
}
.floating-info-tip .tip-block {
  margin: 0 0 8px;
  padding: 8px 10px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
}
.floating-info-tip .tip-badge {
  display: inline-block;
  margin-bottom: 6px;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
}
.floating-info-tip .tip-on .tip-badge {
  background: rgba(34, 197, 94, 0.22);
  color: #86efac;
}
.floating-info-tip .tip-off .tip-badge {
  background: rgba(148, 163, 184, 0.22);
  color: #cbd5e1;
}
.floating-info-tip ul {
  margin: 0;
  padding-left: 1.1em;
  color: #f1f5f9;
}
.floating-info-tip li {
  margin: 2px 0;
}
.floating-info-tip .tip-aside {
  margin: 6px 0 0;
  color: #94a3b8;
  font-size: 11px;
  line-height: 1.45;
}
.floating-info-tip .tip-foot {
  margin: 10px 0 0;
  padding-top: 8px;
  border-top: 1px solid rgba(148, 163, 184, 0.28);
  color: #94a3b8;
  font-size: 11px;
  line-height: 1.5;
}
</style>
