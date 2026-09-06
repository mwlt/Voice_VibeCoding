import type { DeviceConfig, KeyAction, TriggerMode } from "../types";
import { vkDisplayName } from "./vkDisplay";

/** 与 Rust `T1Button::to_id` 对齐的 14 键 */
export const T1_BUTTON_IDS = [
  "power",
  "up",
  "down",
  "left",
  "right",
  "ok",
  "delete",
  "voice",
  "mute",
  "home",
  "mouse",
  "menu",
  "vol_plus",
  "vol_minus",
] as const;

export type T1ButtonId = (typeof T1_BUTTON_IDS)[number];

/** 实物正面可见热区（音量在右侧拨杆，不在正面） */
export const T1_FACE_BUTTON_IDS = [
  "power",
  "up",
  "down",
  "left",
  "right",
  "ok",
  "delete",
  "voice",
  "mute",
  "mouse",
  "home",
  "menu",
] as const;

export const T1_DEFAULT_LABELS: Record<T1ButtonId, string> = {
  power: "电源",
  up: "上",
  down: "下",
  left: "左",
  right: "右",
  ok: "确定",
  delete: "删除",
  voice: "语音",
  mute: "静音",
  home: "主页",
  mouse: "鼠标",
  menu: "菜单",
  vol_plus: "音量+",
  vol_minus: "音量-",
};

/** 映射台左栏（对齐机身左侧） */
export const T1_LEFT_COLUMN_IDS: readonly T1ButtonId[] = [
  "up",
  "left",
  "ok",
  "down",
  "delete",
  "mute",
  "mouse",
];

/** 映射台右栏（对齐机身右侧：电源 → 音量 ± → 右 → 语音 → 主页 → 菜单） */
export const T1_RIGHT_COLUMN_IDS: readonly T1ButtonId[] = [
  "power",
  "vol_plus",
  "vol_minus",
  "right",
  "voice",
  "home",
  "menu",
];

/** 映射台右栏底部 · 语音键快速设置（四种，对齐小米） */
export interface T1VoiceQuickPreset {
  id: string;
  segments: string[];
  vks: number[];
  triggerMode: TriggerMode;
}

export const T1_VOICE_QUICK_PRESETS: readonly T1VoiceQuickPreset[] = [
  {
    id: "ctrl-win",
    segments: ["左 Ctrl", "左 Win"],
    vks: [0xa2, 0x5b],
    triggerMode: "Toggle",
  },
  {
    id: "win-alt",
    segments: ["左 Win", "左 Alt"],
    vks: [0x5b, 0xa4],
    triggerMode: "Toggle",
  },
  {
    id: "ralt-space",
    segments: ["右 Alt", "空格 Space"],
    vks: [0xa5, 0x20],
    triggerMode: "Toggle",
  },
  {
    id: "ralt",
    segments: ["右 Alt"],
    vks: [0xa5],
    triggerMode: "Hold",
  },
];

export function t1VksToHotkeyNames(vks: number[]): string[] {
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

function vksToAction(vks: number[]): KeyAction {
  if (!vks.length) return { type: "None", value: null };
  if (vks.length === 1) return { type: "SingleKey", value: vks[0] };
  return { type: "ComboKey", value: [...vks] };
}

export function applyT1CapturedBinding(
  config: DeviceConfig,
  buttonId: string,
  vks: number[]
): DeviceConfig {
  const action = vksToAction(vks);
  const button_bindings = {
    ...(config.button_bindings || {}),
    [buttonId]: action,
  };
  const next: DeviceConfig = {
    ...config,
    button_bindings,
  };
  if (buttonId === "voice") {
    next.voice_hotkey = t1VksToHotkeyNames(vks);
    // 豆包/千问「长按右 Alt」→ Hold 闩锁（T1 HID 仅为脉冲，软件保持按下直到再按一次）
    // 「免按 右Alt+空格」→ Toggle 点按
    const onlyAlt =
      vks.length === 1 && (vks[0] === 0xa4 || vks[0] === 0xa5 || vks[0] === 0x12);
    if (onlyAlt) next.trigger_mode = "Hold";
    else if (vks.length > 1) next.trigger_mode = "Toggle";
  }
  return next;
}

export function applyT1VoiceQuick(
  config: DeviceConfig,
  preset: T1VoiceQuickPreset
): DeviceConfig {
  const action = vksToAction(preset.vks);
  return {
    ...config,
    button_bindings: {
      ...(config.button_bindings || {}),
      voice: action,
    },
    voice_hotkey: t1VksToHotkeyNames(preset.vks),
    trigger_mode: preset.triggerMode,
    voice_shortcut_enabled: true,
    voice_release_behavior: "None",
  };
}

export function clearT1Binding(
  config: DeviceConfig,
  buttonId: string
): DeviceConfig {
  const button_bindings = {
    ...(config.button_bindings || {}),
    [buttonId]: { type: "None" as const, value: null },
  };
  const next: DeviceConfig = {
    ...config,
    button_bindings,
  };
  if (buttonId === "voice") {
    next.voice_hotkey = [];
  }
  return next;
}

export function t1ActionLabel(action: KeyAction): string {
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

export function t1LabelOf(
  id: string,
  aliases?: Record<string, string> | null
): string {
  return aliases?.[id] || T1_DEFAULT_LABELS[id as T1ButtonId] || id;
}
