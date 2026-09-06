import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const stagePath = join(here, "../components/t1/T1KeyMappingStage.vue");
const hotspotPath = join(here, "../components/t1/T1RemoteHotspot.vue");
const iconPath = join(here, "../components/t1/T1KeyIcon.vue");

/** 禁止的 import / 路径片段（避免误伤 T1RemoteHotspot 等合法名字） */
const FORBIDDEN_IMPORTS = [
  'from "../KeyMappingStage',
  'from "../../components/KeyMappingStage',
  'from "./RemoteHotspot',
  'from "../RemoteHotspot',
  'from "../../components/RemoteHotspot',
  'from "./RemoteKeyIcon',
  'from "../RemoteKeyIcon',
  'from "../../components/RemoteKeyIcon',
  'from "../KeyBindingEditor',
  'from "../../components/KeyBindingEditor',
  "imePreset",
];

describe("T1 mapping components independence", () => {
  it("T1KeyMappingStage does not import Xiaomi/shared mapping components", () => {
    const src = readFileSync(stagePath, "utf8");
    for (const frag of FORBIDDEN_IMPORTS) {
      expect(src, `must not contain ${frag}`).not.toContain(frag);
    }
    expect(src).toContain("applyT1CapturedBinding");
    expect(src).toContain("applyT1VoiceQuick");
    expect(src).toContain("clearT1Binding");
    expect(src).toContain("T1RemoteHotspot");
    expect(src).toContain("T1KeyIcon");
    expect(src).toContain("voice-quick-setup");
  });

  it("T1RemoteHotspot and T1KeyIcon stay self-contained", () => {
    const hotspot = readFileSync(hotspotPath, "utf8");
    const icon = readFileSync(iconPath, "utf8");
    for (const frag of FORBIDDEN_IMPORTS) {
      expect(hotspot).not.toContain(frag);
      expect(icon).not.toContain(frag);
    }
    expect(hotspot).toContain('data-key-id="delete"');
    expect(hotspot).toContain('data-key-id="voice"');
    expect(hotspot).toContain('data-key-id="vol_plus"');
    expect(hotspot).toContain('data-key-id="vol_minus"');
    expect(icon).toContain('keyId === "vol_plus"');
  });
});
