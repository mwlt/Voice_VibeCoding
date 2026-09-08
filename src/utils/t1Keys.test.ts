import { describe, expect, it } from "vitest";
import type { DeviceConfig } from "../types";
import {
  T1_BUTTON_IDS,
  T1_DEFAULT_LABELS,
  T1_FACE_BUTTON_IDS,
  T1_BLE_FIXED_SYSTEM_IDS,
  T1_USB_FIXED_SYSTEM_IDS,
  T1_BLE_LEFT_COLUMN_IDS,
  T1_BLE_RIGHT_COLUMN_IDS,
  T1_USB_LEFT_COLUMN_IDS,
  T1_USB_RIGHT_COLUMN_IDS,
  T1_FIXED_SYSTEM_HINTS,
  T1_VOICE_QUICK_PRESETS,
  applyT1CapturedBinding,
  applyT1VoiceQuick,
  clearT1Binding,
  stripT1FixedBindings,
  t1ActionLabel,
  t1IsFixedSystemKey,
  t1LeftColumnIds,
  t1RightColumnIds,
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
      delete: { type: "SingleKey", value: 0x08 },
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

  it("BLE mapping columns only show bindable keys", () => {
    expect(T1_FACE_BUTTON_IDS).not.toContain("vol_plus");
    expect(T1_FACE_BUTTON_IDS).not.toContain("vol_minus");
    expect(T1_FACE_BUTTON_IDS).toHaveLength(12);

    const cols = [...T1_BLE_LEFT_COLUMN_IDS, ...T1_BLE_RIGHT_COLUMN_IDS];
    expect(cols.sort()).toEqual(["delete", "home", "menu", "voice"].sort());
    expect(new Set(cols).size).toBe(4);
    for (const id of cols) {
      expect(t1IsFixedSystemKey(id, "ble")).toBe(false);
    }
  });

  it("USB mapping columns keep full remote layout", () => {
    const cols = [
      ...T1_USB_LEFT_COLUMN_IDS,
      ...T1_USB_RIGHT_COLUMN_IDS,
    ];
    expect(cols).toEqual([
      "up",
      "left",
      "ok",
      "down",
      "delete",
      "mute",
      "mouse",
      "power",
      "vol_plus",
      "vol_minus",
      "right",
      "voice",
      "home",
      "menu",
    ]);
    expect(t1LeftColumnIds("usb")).toEqual(T1_USB_LEFT_COLUMN_IDS);
    expect(t1RightColumnIds("usb")).toEqual(T1_USB_RIGHT_COLUMN_IDS);
    expect(t1IsFixedSystemKey("ok", "usb")).toBe(false);
    expect(t1IsFixedSystemKey("mute", "usb")).toBe(false);
    expect(t1IsFixedSystemKey("power", "usb")).toBe(true);
    expect(t1IsFixedSystemKey("mouse", "usb")).toBe(true);
  });

  it("marks BLE passthrough face/media keys as fixed", () => {
    for (const id of T1_BLE_FIXED_SYSTEM_IDS) {
      expect(t1IsFixedSystemKey(id, "ble")).toBe(true);
      expect(T1_FIXED_SYSTEM_HINTS[id]).toContain("不可绑定");
    }
    expect(t1IsFixedSystemKey("ok", "ble")).toBe(true);
    expect(t1IsFixedSystemKey("delete", "ble")).toBe(false);
    expect([...T1_USB_FIXED_SYSTEM_IDS]).toEqual(["power", "mouse"]);
  });

  it("stripT1FixedBindings only clears transport-fixed keys", () => {
    const ble = stripT1FixedBindings(baseConfig(), "ble");
    expect(ble.button_bindings.ok).toEqual({ type: "None", value: null });
    expect(ble.button_bindings.delete).toEqual({
      type: "SingleKey",
      value: 0x08,
    });

    const usb = stripT1FixedBindings(baseConfig(), "usb");
    expect(usb.button_bindings.ok).toEqual({ type: "SingleKey", value: 0x0d });
    expect(usb.button_bindings.power).toEqual({ type: "None", value: null });
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

  it("applies voice binding without wiping USB ok binding", () => {
    const hold = applyT1VoiceQuick(baseConfig(), T1_VOICE_QUICK_PRESETS[0], "usb");
    expect(hold.button_bindings.voice).toEqual({
      type: "ComboKey",
      value: [0xa2, 0x5b],
    });
    expect(hold.button_bindings.ok).toEqual({ type: "SingleKey", value: 0x0d });
    expect(hold.voice_hotkey).toEqual(["leftctrl", "leftwin"]);

    const ble = applyT1VoiceQuick(baseConfig(), T1_VOICE_QUICK_PRESETS[0], "ble");
    expect(ble.button_bindings.ok).toEqual({ type: "None", value: null });
  });
});

describe("applyT1CapturedBinding", () => {
  it("refuses capture on BLE fixed keys (ok)", () => {
    const base = baseConfig();
    const next = applyT1CapturedBinding(base, "ok", [0x0d], "ble");
    expect(next).toBe(base);
  });

  it("allows capture on USB ok", () => {
    const next = applyT1CapturedBinding(baseConfig(), "ok", [0x20], "usb");
    expect(next.button_bindings.ok).toEqual({
      type: "SingleKey",
      value: 0x20,
    });
  });

  it("writes ComboKey for home", () => {
    const next = applyT1CapturedBinding(baseConfig(), "home", [0x5b, 0x0d], "usb");
    expect(next.button_bindings.home).toEqual({
      type: "ComboKey",
      value: [0x5b, 0x0d],
    });
  });

  it("syncs voice_hotkey only for voice, never mic", () => {
    const next = applyT1CapturedBinding(baseConfig(), "voice", [0xa5], "ble");
    expect(next.button_bindings.voice).toEqual({
      type: "SingleKey",
      value: 0xa5,
    });
    expect(next.button_bindings.mic).toBeUndefined();
    expect(next.voice_hotkey).toEqual(["rightalt"]);
    expect(next.trigger_mode).toBe("Hold");
  });

  it("sets Toggle when voice is a chord", () => {
    const next = applyT1CapturedBinding(baseConfig(), "voice", [0xa5, 0x20], "ble");
    expect(next.voice_hotkey).toEqual(["rightalt", "space"]);
    expect(next.trigger_mode).toBe("Toggle");
  });

  it("ignores capture on BLE mute (fixed)", () => {
    const base = baseConfig();
    const next = applyT1CapturedBinding(base, "mute", [], "ble");
    expect(next).toBe(base);
  });

  it("ignores capture/clear for power/mouse on both transports", () => {
    const base = baseConfig();
    expect(applyT1CapturedBinding(base, "power", [0x1b], "usb")).toBe(base);
    expect(clearT1Binding(base, "mouse", "ble")).toBe(base);
  });
});

describe("clearT1Binding", () => {
  it("clears binding and voice_hotkey when clearing voice", () => {
    const cfg = applyT1CapturedBinding(baseConfig(), "voice", [0xa5, 0x20], "usb");
    const next = clearT1Binding(cfg, "voice", "usb");
    expect(next.button_bindings.voice).toEqual({ type: "None", value: null });
    expect(next.voice_hotkey).toEqual([]);
  });

  it("clears delete without wiping voice_hotkey", () => {
    const next = clearT1Binding(baseConfig(), "delete", "usb");
    expect(next.button_bindings.delete).toEqual({ type: "None", value: null });
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
