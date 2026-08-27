import { describe, expect, it } from "vitest";
import type { DeviceConfig } from "../types";
import { IME_PRESETS, applyImePresetConfig, listImePresets } from "./imePreset";

function baseConfig(): DeviceConfig {
  return {
    button_aliases: {},
    button_bindings: {
      mic: { type: "SingleKey", value: 0xa5 },
    },
    voice_hotkey: ["rightalt"],
    trigger_mode: "Toggle",
    bluetooth_address: null,
    voice_shortcut_enabled: false,
  };
}

describe("applyImePresetConfig", () => {
  it("applies wechat hold as Ctrl+Win Hold with no release tap", () => {
    const next = applyImePresetConfig(baseConfig(), "wechat-hold");
    expect(next.voice_hotkey).toEqual(["leftctrl", "leftwin"]);
    expect(next.trigger_mode).toBe("Hold");
    expect(next.voice_shortcut_enabled).toBe(true);
    expect(next.voice_release_behavior).toBe("None");
    expect(next.button_bindings.mic).toEqual({
      type: "ComboKey",
      value: [0xa2, 0x5b],
    });
    expect(next.button_bindings.voice).toEqual({
      type: "ComboKey",
      value: [0xa2, 0x5b],
    });
  });

  it("applies doubao hands-free as RightAlt+Space Toggle", () => {
    const next = applyImePresetConfig(baseConfig(), "doubao-hands-free");
    expect(next.voice_hotkey).toEqual(["rightalt", "space"]);
    expect(next.trigger_mode).toBe("Toggle");
    expect(next.voice_release_behavior).toBe("None");
    expect(next.button_bindings.mic).toEqual({
      type: "ComboKey",
      value: [0xa5, 0x20],
    });
  });

  it("applies qianwen win+alt as Hold combo", () => {
    const next = applyImePresetConfig(baseConfig(), "qianwen-win-alt");
    expect(next.voice_hotkey).toEqual(["leftwin", "leftalt"]);
    expect(next.trigger_mode).toBe("Hold");
    expect(next.button_bindings.mic).toEqual({
      type: "ComboKey",
      value: [0x5b, 0xa4],
    });
  });

  it("lists all presets with stable ids", () => {
    const ids = listImePresets().map((p) => p.id);
    expect(ids).toEqual([
      "wechat-hold",
      "doubao-hold",
      "qianwen-ctrl-win",
      "qianwen-win-alt",
      "qianwen-hold",
    ]);
    expect(Object.keys(IME_PRESETS).sort()).toEqual(
      [...ids, "doubao-hands-free"].sort(),
    );
  });
});
