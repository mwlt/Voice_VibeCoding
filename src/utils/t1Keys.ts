import type { DeviceConfig, KeyAction, TriggerMode } from "../types";
import { vkDisplayName } from "./vkDisplay";

export type T1Transport = "ble" | "usb";

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

/**
 * USB：仅电源/鼠标机内固定（仍展示说明卡，不可录入）。
 * BLE：另含方向/OK/音量/静音等系统透传键（映射台直接不展示）。
 */
export const T1_USB_FIXED_SYSTEM_IDS = ["power", "mouse"] as const;

export const T1_BLE_FIXED_SYSTEM_IDS = [
  "power",
  "mouse",
  "up",
  "down",
  "left",
  "right",
  "ok",
  "mute",
  "vol_plus",
  "vol_minus",
] as const;

/** @deprecated 用 t1FixedSystemIds(transport)；默认 BLE 透传集合 */
export const T1_FIXED_SYSTEM_IDS = T1_BLE_FIXED_SYSTEM_IDS;

export type T1FixedSystemId = (typeof T1_BLE_FIXED_SYSTEM_IDS)[number];

export const T1_FIXED_SYSTEM_HINTS: Record<T1FixedSystemId, string> = {
  power: "系统电源 · 不可绑定",
  mouse: "遥控器空鼠 · 不可绑定",
  up: "系统原生方向 · 不可绑定",
  down: "系统原生方向 · 不可绑定",
  left: "系统原生方向 · 不可绑定",
  right: "系统原生方向 · 不可绑定",
  ok: "系统原生确定 · 不可绑定",
  mute: "系统原生静音 · 不可绑定",
  vol_plus: "系统原生音量 · 不可绑定",
  vol_minus: "系统原生音量 · 不可绑定",
};

/** USB 映射台左栏（与拆分前一致） */
export const T1_USB_LEFT_COLUMN_IDS: readonly T1ButtonId[] = [
  "up",
  "left",
  "ok",
  "down",
  "delete",
  "mute",
  "mouse",
];

/** USB 映射台右栏 */
export const T1_USB_RIGHT_COLUMN_IDS: readonly T1ButtonId[] = [
  "power",
  "vol_plus",
  "vol_minus",
  "right",
  "voice",
  "home",
  "menu",
];

/** BLE：仅可映射键 */
export const T1_BLE_LEFT_COLUMN_IDS: readonly T1ButtonId[] = ["delete"];

export const T1_BLE_RIGHT_COLUMN_IDS: readonly T1ButtonId[] = [
  "voice",
  "home",
  "menu",
];

/** @deprecated 默认 BLE 列；组件请用 t1Left/RightColumnIds(transport) */
export const T1_LEFT_COLUMN_IDS = T1_BLE_LEFT_COLUMN_IDS;
export const T1_RIGHT_COLUMN_IDS = T1_BLE_RIGHT_COLUMN_IDS;

export function t1FixedSystemIds(
  transport: T1Transport
): readonly string[] {
  return transport === "usb"
    ? T1_USB_FIXED_SYSTEM_IDS
    : T1_BLE_FIXED_SYSTEM_IDS;
}

export function t1LeftColumnIds(
  transport: T1Transport
): readonly T1ButtonId[] {
  return transport === "usb"
    ? T1_USB_LEFT_COLUMN_IDS
    : T1_BLE_LEFT_COLUMN_IDS;
}

export function t1RightColumnIds(
  transport: T1Transport
): readonly T1ButtonId[] {
  return transport === "usb"
    ? T1_USB_RIGHT_COLUMN_IDS
    : T1_BLE_RIGHT_COLUMN_IDS;
}

export function t1IsFixedSystemKey(
  id: string,
  transport: T1Transport = "ble"
): id is T1FixedSystemId {
  return t1FixedSystemIds(transport).includes(id);
}

/** 清掉该传输下固定键上的历史绑定（未绑定） */
export function stripT1FixedBindings(
  config: DeviceConfig,
  transport: T1Transport = "ble"
): DeviceConfig {
  const button_bindings = { ...(config.button_bindings || {}) };
  for (const id of t1FixedSystemIds(transport)) {
    button_bindings[id] = { type: "None", value: null };
  }
  return { ...config, button_bindings };
}

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
  vks: number[],
  transport: T1Transport = "ble"
): DeviceConfig {
  if (t1IsFixedSystemKey(buttonId, transport)) return config;
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
    const onlyAlt =
      vks.length === 1 && (vks[0] === 0xa4 || vks[0] === 0xa5 || vks[0] === 0x12);
    if (onlyAlt) next.trigger_mode = "Hold";
    else if (vks.length > 1) next.trigger_mode = "Toggle";
  }
  return stripT1FixedBindings(next, transport);
}

export function applyT1VoiceQuick(
  config: DeviceConfig,
  preset: T1VoiceQuickPreset,
  transport: T1Transport = "ble"
): DeviceConfig {
  const action = vksToAction(preset.vks);
  return stripT1FixedBindings(
    {
      ...config,
      button_bindings: {
        ...(config.button_bindings || {}),
        voice: action,
      },
      voice_hotkey: t1VksToHotkeyNames(preset.vks),
      trigger_mode: preset.triggerMode,
      voice_shortcut_enabled: true,
      voice_release_behavior: "None",
    },
    transport
  );
}

export function clearT1Binding(
  config: DeviceConfig,
  buttonId: string,
  transport: T1Transport = "ble"
): DeviceConfig {
  if (t1IsFixedSystemKey(buttonId, transport)) return config;
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
  return stripT1FixedBindings(next, transport);
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
  if (aliases?.[id]) return aliases[id];
  return T1_DEFAULT_LABELS[id as T1ButtonId] || id;
}
