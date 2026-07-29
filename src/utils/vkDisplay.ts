/** VK → 显示名（与 Rust `vk_to_label` / Python NAMED_KEYS 对齐） */
export function vkDisplayName(vk: number): string {
  const map: Record<number, string> = {
    0x08: "Backspace",
    0x09: "Tab",
    0x0d: "Enter",
    0x13: "Pause",
    0x14: "CapsLock",
    0x1b: "Esc",
    0x20: "Space",
    0x21: "PageUp",
    0x22: "PageDown",
    0x23: "End",
    0x24: "Home",
    0x25: "←",
    0x26: "↑",
    0x27: "→",
    0x28: "↓",
    0x2c: "PrtSc",
    0x2d: "Insert",
    0x2e: "Delete",
    0x5d: "Menu",
    0x90: "NumLock",
    0x91: "ScrLk",
    0x6a: "Num*",
    0x6b: "Num+",
    0x6d: "Num-",
    0x6e: "Num.",
    0x6f: "Num/",
    0x10: "左 Shift",
    0xa0: "左 Shift",
    0xa1: "右 Shift",
    0x11: "左 Ctrl",
    0xa2: "左 Ctrl",
    0xa3: "右 Ctrl",
    0x12: "左 Alt",
    0xa4: "左 Alt",
    0xa5: "右 Alt",
    0x5b: "左 Win",
    0x5c: "右 Win",
    0xad: "静音",
    0xae: "音量-",
    0xaf: "音量+",
    0xb0: "下一曲",
    0xb1: "上一曲",
    0xb2: "停止",
    0xb3: "播放/暂停",
    0xb7: "计算器",
    0xba: ";",
    0xbb: "=",
    0xbc: ",",
    0xbd: "-",
    0xbe: ".",
    0xbf: "/",
    0xc0: "`",
    0xdb: "[",
    0xdc: "\\",
    0xdd: "]",
    0xde: "'",
  };
  if (map[vk]) return map[vk];
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk);
  if (vk >= 0x30 && vk <= 0x39) return String(vk - 0x30);
  if (vk >= 0x60 && vk <= 0x69) return `Num${vk - 0x60}`;
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  return `VK_0x${vk.toString(16).toUpperCase()}`;
}

/** 录入 UI 常驻媒体/系统键兜底 */
export const MEDIA_PICK_KEYS: { vk: number; label: string }[] = [
  { vk: 0xaf, label: "音量+" },
  { vk: 0xae, label: "音量-" },
  { vk: 0xad, label: "静音" },
  { vk: 0xb7, label: "计算器" },
];
