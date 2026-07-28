// 前端类型定义

export type BridgeType = "xiaomi" | "t1" | "hanvon";

export type BridgeStatus =
  | "Disconnected"
  | "Connecting"
  | "Connected"
  | "Error"
  | `Error|${string}`
  | `Error: ${string}`;

export interface DeviceInfo {
  bridge_type: BridgeType;
  status: BridgeStatus;
  device_name: string | null;
  device_address: string | null;
  battery_level: number | null;
}

export interface KeyAction {
  type: "SingleKey" | "ComboKey" | "TextInput" | "LaunchApp" | "None";
  value: number | number[] | string | null;
}

export type TriggerMode = "Toggle" | "Hold";
/** Toggle=点击型快捷键；Hold=按住型快捷键（传声仍为按住遥控语音键） */

export interface DeviceConfig {
  button_aliases: Record<string, string>;
  button_bindings: Record<string, KeyAction>;
  voice_hotkey: string[] | null;
  trigger_mode: TriggerMode;
  bluetooth_address: string | null;
  /** 麦克风增益 dB（对齐 Python gain_db，默认 10） */
  gain_db?: number;
  /** 是否注入语音快捷键（传声与此项无关） */
  voice_shortcut_enabled?: boolean;
}

export interface GlobalSettings {
  autostart: boolean;
  language: string;
  minimize_to_tray: boolean;
  ignored_update_version?: string | null;
}

export interface AppUpdateInfo {
  checked: boolean;
  updateAvailable: boolean;
  ignored: boolean;
  currentVersion: string;
  latestVersion: string;
  notes: string;
  giteePage: string;
  githubPage: string;
  setupUrl: string;
  source: string;
  error?: string | null;
}

export interface AudioDevice {
  name: string;
  id: string;
  is_input: boolean;
  is_default: boolean;
}
