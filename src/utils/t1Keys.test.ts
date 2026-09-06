import { describe, expect, it } from "vitest";
import type { DeviceConfig } from "../types";
import {
  T1_BUTTON_IDS,
  T1_DEFAULT_LABELS,
  T1_FACE_BUTTON_IDS,
  T1_FIXED_SYSTEM_HINTS,
  T1_LEFT_COLUMN_IDS,
  T1_RIGHT_COLUMN_IDS,
  T1_VOICE_QUICK_PRESETS,
  applyT1CapturedBinding,
  applyT1VoiceQuick,
  clearT1Binding,
  t1ActionLabel,
  t1IsFixedSystemKey,
  t1VksToHotkeyNames,
} from "./t1Keys";

/** 与 Rust `T1Button::to_id` 对齐的独立字面量（勿从实现再导出） */
const BACKEND_T1_IDS = [
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

function baseConfig(): DeviceConfig {
  return {
    button_aliases: { ...T1_DEFAULT_LABELS },
    button_bindings: {
      ok: { type: "SingleKey", value: 0x0d },
    },
    voice_hotkey: ["rightalt"],
    trigger_mode: "Toggle",
    bluetooth_address: null,
  };
}

describe("T1 button registry", () => {
  it("exposes exactly the 14 backend button ids", () => {
    expect([...T1_BUTTON_IDS].sort()).toEqual([...BACKEND_T1_IDS].sort());
    expect(T1_BUTTON_IDS).toHaveLength(14);
  });

  it("labels cover every button id", () => {
    for (const id of BACKEND_T1_IDS) {
      expect(T1_DEFAULT_LABELS[id]).toBeTruthy();
    }
  });

  it("face ids exclude side volume; columns cover all 14 without overlap", () => {
    expect(T1_FACE_BUTTON_IDS).not.toContain("vol_plus");
    expect(T1_FACE_BUTTON_IDS).not.toContain("vol_minus");
    expect(T1_FACE_BUTTON_IDS).toHaveLength(12);

    const cols = [...T1_LEFT_COLUMN_IDS, ...T1_RIGHT_COLUMN_IDS];
    expect(cols.sort()).toEqual([...BACKEND_T1_IDS].sort());
    expect(new Set(cols).size).toBe(14);
    expect(T1_LEFT_COLUMN_IDS).not.toContain("power");
    expect(T1_RIGHT_COLUMN_IDS).toEqual([
      "power",
      "vol_plus",
      "vol_minus",
      "right",
      "voice",
      "home",
      "menu",
    ]);
  });

  it("marks power and mouse as fixed system keys (not bindable)", () => {
    expect(t1IsFixedSystemKey("power")).toBe(true);
    expect(t1IsFixedSystemKey("mouse")).toBe(true);
    expect(t1IsFixedSystemKey("ok")).toBe(false);
    expect(T1_FIXED_SYSTEM_HINTS.power).toContain("不可绑定");
    expect(T1_FIXED_SYSTEM_HINTS.mouse).toContain("不可绑定");
  });
});

describe("T1 voice quick presets", () => {
  it("exposes four Xiaomi-aligned presets", () => {
    expect(T1_VOICE_QUICK_PRESETS).toHaveLength(4);
    expect(T1_VOICE_QUICK_PRESETS.map((p) => p.id)).toEqual([
      "ctrl-win",
      "win-alt",
      "ralt-space",
      "ralt",
    ]);
  });

  it("applies voice binding, hotkey and trigger mode without writing mic", () => {
    const hold = applyT1VoiceQuick(baseConfig(), T1_VOICE_QUICK_PRESETS[0]);
    expect(hold.button_bindings.voice).toEqual({
      type: "ComboKey",
      value: [0xa2, 0x5b],
    });
    expect(hold.button_bindings.mic).toBeUndefined();
    expect(hold.voice_hotkey).toEqual(["leftctrl", "leftwin"]);
    expect(hold.trigger_mode).toBe("Toggle");

    const toggle = applyT1VoiceQuick(baseConfig(), T1_VOICE_QUICK_PRESETS[2]);
    expect(toggle.voice_hotkey).toEqual(["rightalt", "space"]);
    expect(toggle.trigger_mode).toBe("Toggle");
  });
});

describe("applyT1CapturedBinding", () => {
  it("writes SingleKey and does not invent mic binding", () => {
    const next = applyT1CapturedBinding(baseConfig(), "ok", [0x0d]);
    expect(next.button_bindings.ok).toEqual({ type: "SingleKey", value: 0x0d });
    expect(next.button_bindings.mic).toBeUndefined();
  });

  it("writes ComboKey for multi-vk", () => {
    const next = applyT1CapturedBinding(baseConfig(), "home", [0x5b, 0x0d]);
    expect(next.button_bindings.home).toEqual({
      type: "ComboKey",
      value: [0x5b, 0x0d],
    });
  });

  it("syncs voice_hotkey only for voice, never mic", () => {
    const next = applyT1CapturedBinding(baseConfig(), "voice", [0xa5]);
    expect(next.button_bindings.voice).toEqual({
      type: "SingleKey",
      value: 0xa5,
    });
    expect(next.button_bindings.mic).toBeUndefined();
    expect(next.voice_hotkey).toEqual(["rightalt"]);
    expect(next.trigger_mode).toBe("Hold");
  });

  it("sets Toggle when voice is a chord (豆包免按)", () => {
    const next = applyT1CapturedBinding(baseConfig(), "voice", [0xa5, 0x20]);
    expect(next.voice_hotkey).toEqual(["rightalt", "space"]);
    expect(next.trigger_mode).toBe("Toggle");
  });

  it("clears empty capture to None without touching voice_hotkey for non-voice", () => {
    const next = applyT1CapturedBinding(baseConfig(), "mute", []);
    expect(next.button_bindings.mute).toEqual({ type: "None", value: null });
    expect(next.voice_hotkey).toEqual(["rightalt"]);
  });

  it("ignores capture/clear for fixed system keys", () => {
    const base = baseConfig();
    const afterCapture = applyT1CapturedBinding(base, "power", [0x1b]);
    expect(afterCapture).toBe(base);
    const afterClear = clearT1Binding(base, "mouse");
    expect(afterClear).toBe(base);
  });
});

describe("clearT1Binding", () => {
  it("clears binding and voice_hotkey when clearing voice", () => {
    const cfg = applyT1CapturedBinding(baseConfig(), "voice", [0xa5, 0x20]);
    const next = clearT1Binding(cfg, "voice");
    expect(next.button_bindings.voice).toEqual({ type: "None", value: null });
    expect(next.button_bindings.mic).toBeUndefined();
    expect(next.voice_hotkey).toEqual([]);
  });

  it("clears non-voice without wiping voice_hotkey", () => {
    const next = clearT1Binding(baseConfig(), "ok");
    expect(next.button_bindings.ok).toEqual({ type: "None", value: null });
    expect(next.voice_hotkey).toEqual(["rightalt"]);
  });
});

describe("t1 helpers", () => {
  it("maps known vks to hotkey names", () => {
    expect(t1VksToHotkeyNames([0xa5, 0x20])).toEqual(["rightalt", "space"]);
  });

  it("formats action labels", () => {
    expect(t1ActionLabel({ type: "None", value: null })).toBe("未绑定");
    expect(t1ActionLabel({ type: "SingleKey", value: 0x0d })).toMatch(/Enter/);
    expect(
      t1ActionLabel({ type: "ComboKey", value: [0xa5, 0x20] })
    ).toContain("+");
  });
});
