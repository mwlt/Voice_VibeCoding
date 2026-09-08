/** Windows 标准配列（主区 + 导航区 + 小键盘），参考 108 键布局。 */

export type KbTone = "alpha" | "mod";

export type KbKey = {
  code: string;
  label: string;
  /** 相对单位宽度，默认 1 */
  w?: number;
  /** 相对单位高度，默认 1（小键盘 + / Enter） */
  h?: number;
  /** 占位（无键） */
  gap?: boolean;
  /**
   * 键帽配色：alpha=白底字符键；mod=浅灰蓝功能/修饰键。
   * 未指定时按 code 推断。
   */
  tone?: KbTone;
  /** 媒体键等用 SVG 图标代替文字 */
  icon?: "mute" | "vol-down" | "vol-up" | "calc";
};

export type KbRow = KbKey[];

const MOD_CODES = new Set([
  "Escape",
  "F1",
  "F2",
  "F3",
  "F4",
  "F5",
  "F6",
  "F7",
  "F8",
  "F9",
  "F10",
  "F11",
  "F12",
  "Tab",
  "CapsLock",
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "MetaLeft",
  "MetaRight",
  "AltLeft",
  "AltRight",
  "ContextMenu",
  "Backspace",
  "Enter",
  "PrintScreen",
  "ScrollLock",
  "Pause",
  "Insert",
  "Home",
  "PageUp",
  "Delete",
  "End",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "NumLock",
  "NumpadDivide",
  "NumpadMultiply",
  "NumpadSubtract",
  "NumpadAdd",
  "NumpadEnter",
  "AudioVolumeMute",
  "AudioVolumeDown",
  "AudioVolumeUp",
  "LaunchApp2",
  "BrowserHome",
  "BrowserBack",
  "MediaPlayPause",
  "MediaTrackNext",
  "MediaTrackPrevious",
]);

export function keyTone(key: KbKey): KbTone {
  if (key.tone) return key.tone;
  if (!key.code || key.gap) return "mod";
  return MOD_CODES.has(key.code) ? "mod" : "alpha";
}

export const KEYBOARD_MAIN: KbRow[] = [
  [
    { code: "Escape", label: "Esc" },
    { code: "F1", label: "F1" },
    { code: "F2", label: "F2" },
    { code: "F3", label: "F3" },
    { code: "F4", label: "F4" },
    { code: "F5", label: "F5" },
    { code: "F6", label: "F6" },
    { code: "F7", label: "F7" },
    { code: "F8", label: "F8" },
    { code: "F9", label: "F9" },
    { code: "F10", label: "F10" },
    { code: "F11", label: "F11" },
    { code: "F12", label: "F12" },
  ],
  [
    { code: "Backquote", label: "`" },
    { code: "Digit1", label: "1" },
    { code: "Digit2", label: "2" },
    { code: "Digit3", label: "3" },
    { code: "Digit4", label: "4" },
    { code: "Digit5", label: "5" },
    { code: "Digit6", label: "6" },
    { code: "Digit7", label: "7" },
    { code: "Digit8", label: "8" },
    { code: "Digit9", label: "9" },
    { code: "Digit0", label: "0" },
    { code: "Minus", label: "-" },
    { code: "Equal", label: "=" },
    { code: "Backspace", label: "Back", w: 2 },
  ],
  [
    { code: "Tab", label: "Tab", w: 1.5 },
    { code: "KeyQ", label: "Q" },
    { code: "KeyW", label: "W" },
    { code: "KeyE", label: "E" },
    { code: "KeyR", label: "R" },
    { code: "KeyT", label: "T" },
    { code: "KeyY", label: "Y" },
    { code: "KeyU", label: "U" },
    { code: "KeyI", label: "I" },
    { code: "KeyO", label: "O" },
    { code: "KeyP", label: "P" },
    { code: "BracketLeft", label: "[" },
    { code: "BracketRight", label: "]" },
    { code: "Backslash", label: "\\", w: 1.5 },
  ],
  [
    { code: "CapsLock", label: "Caps", w: 1.75 },
    { code: "KeyA", label: "A" },
    { code: "KeyS", label: "S" },
    { code: "KeyD", label: "D" },
    { code: "KeyF", label: "F" },
    { code: "KeyG", label: "G" },
    { code: "KeyH", label: "H" },
    { code: "KeyJ", label: "J" },
    { code: "KeyK", label: "K" },
    { code: "KeyL", label: "L" },
    { code: "Semicolon", label: ";" },
    { code: "Quote", label: "'" },
    { code: "Enter", label: "Enter", w: 2.25 },
  ],
  [
    { code: "ShiftLeft", label: "Shift", w: 2.25 },
    { code: "KeyZ", label: "Z" },
    { code: "KeyX", label: "X" },
    { code: "KeyC", label: "C" },
    { code: "KeyV", label: "V" },
    { code: "KeyB", label: "B" },
    { code: "KeyN", label: "N" },
    { code: "KeyM", label: "M" },
    { code: "Comma", label: "," },
    { code: "Period", label: "." },
    { code: "Slash", label: "/" },
    { code: "ShiftRight", label: "Shift", w: 2.75 },
  ],
  [
    { code: "ControlLeft", label: "Ctrl", w: 1.25 },
    { code: "MetaLeft", label: "Win", w: 1.25 },
    { code: "AltLeft", label: "Alt", w: 1.25 },
    { code: "Space", label: "", w: 6.25 },
    { code: "AltRight", label: "Alt", w: 1.25 },
    { code: "MetaRight", label: "Win", w: 1.25 },
    { code: "ContextMenu", label: "Menu", w: 1.25 },
    { code: "ControlRight", label: "Ctrl", w: 1.25 },
  ],
];

export const KEYBOARD_NAV: KbRow[] = [
  [
    { code: "PrintScreen", label: "PrtSc" },
    { code: "ScrollLock", label: "ScrLk" },
    { code: "Pause", label: "Pause" },
  ],
  [
    { code: "Insert", label: "Ins" },
    { code: "Home", label: "Home" },
    { code: "PageUp", label: "PgUp" },
  ],
  [
    { code: "Delete", label: "Del" },
    { code: "End", label: "End" },
    { code: "PageDown", label: "PgDn" },
  ],
  [{ code: "", label: "", gap: true }, { code: "", label: "", gap: true }, { code: "", label: "", gap: true }],
  [
    { code: "", label: "", gap: true },
    { code: "ArrowUp", label: "↑" },
    { code: "", label: "", gap: true },
  ],
  [
    { code: "ArrowLeft", label: "←" },
    { code: "ArrowDown", label: "↓" },
    { code: "ArrowRight", label: "→" },
  ],
];

/** 108 键：小键盘上方多媒体键（界面用图标，label 仅作无障碍） */
export const KEYBOARD_NUMPAD_MEDIA: KbKey[] = [
  { code: "AudioVolumeMute", label: "Mute", icon: "mute" },
  { code: "AudioVolumeDown", label: "Vol-", icon: "vol-down" },
  { code: "AudioVolumeUp", label: "Vol+", icon: "vol-up" },
  { code: "LaunchApp2", label: "Calc", icon: "calc" },
];

/**
 * 108 键小键盘（CSS grid 4 列）。
 * + / Enter 跨两行；0 跨两列。
 */
export const KEYBOARD_NUMPAD: KbKey[] = [
  { code: "NumLock", label: "Num" },
  { code: "NumpadDivide", label: "/" },
  { code: "NumpadMultiply", label: "*" },
  { code: "NumpadSubtract", label: "-" },
  { code: "Numpad7", label: "7" },
  { code: "Numpad8", label: "8" },
  { code: "Numpad9", label: "9" },
  { code: "NumpadAdd", label: "+", h: 2 },
  { code: "Numpad4", label: "4" },
  { code: "Numpad5", label: "5" },
  { code: "Numpad6", label: "6" },
  { code: "Numpad1", label: "1" },
  { code: "Numpad2", label: "2" },
  { code: "Numpad3", label: "3" },
  { code: "NumpadEnter", label: "Ent", h: 2 },
  { code: "Numpad0", label: "0", w: 2 },
  { code: "NumpadDecimal", label: "." },
];

/** 其它遥控常见键（音量/计算器已在小键盘上方） */
export const KEYBOARD_MEDIA: KbKey[] = [
  { code: "BrowserHome", label: "Browser Home" },
  { code: "BrowserBack", label: "Browser Back" },
  { code: "MediaPlayPause", label: "Play/Pause" },
  { code: "MediaTrackNext", label: "Next" },
  { code: "MediaTrackPrevious", label: "Prev" },
];

/** 将 KeyboardEvent 规范成稳定 code（部分媒体键 code 为空） */
export function resolveEventCode(e: KeyboardEvent): string {
  if (e.code) return e.code;
  const key = e.key;
  if (key === "AudioVolumeMute" || key === "VolumeMute") return "AudioVolumeMute";
  if (key === "AudioVolumeDown" || key === "VolumeDown") return "AudioVolumeDown";
  if (key === "AudioVolumeUp" || key === "VolumeUp") return "AudioVolumeUp";
  if (key === "LaunchApp2" || key === "LaunchApplication2") return "LaunchApp2";
  if (key === "BrowserHome") return "BrowserHome";
  if (key === "BrowserBack") return "BrowserBack";
  if (key === "MediaPlayPause") return "MediaPlayPause";
  if (key === "MediaTrackNext") return "MediaTrackNext";
  if (key === "MediaTrackPrevious") return "MediaTrackPrevious";
  if (key === "ContextMenu") return "ContextMenu";
  // VK_LAUNCH_APP2 = 0xB7 (183) 计算器
  if (e.keyCode === 183) return "LaunchApp2";
  return key || `KeyCode${e.keyCode}`;
}

export function formatKeyLabel(e: KeyboardEvent): string {
  if (e.key === " ") return "Space";
  if (e.key.length === 1) return e.key;
  return e.key;
}
