import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const t1SettingsPath = join(here, "../views/T1Settings.vue");

describe("T1Settings wiring", () => {
  it("uses T1KeyMappingStage and not KeyBindingEditor", () => {
    const src = readFileSync(t1SettingsPath, "utf8");
    expect(src).toContain("T1KeyMappingStage");
    expect(src).toContain("components/t1/T1KeyMappingStage.vue");
    expect(src).toContain("按键映射");
    expect(src).not.toContain("KeyBindingEditor");
    expect(src).not.toContain("components/KeyMappingStage");
    expect(src).not.toContain("components/RemoteHotspot");
  });

  it("wires independent BLE connect and status IPC", () => {
    const src = readFileSync(t1SettingsPath, "utf8");
    expect(src).toContain("start_t1_ble_bridge");
    expect(src).toContain("stop_t1_ble_bridge");
    expect(src).toContain("t1_ble_running");
    expect(src).toContain("蓝牙连接");
    expect(src).toContain('listen<{');
    expect(src).toContain('"t1-ble"');
  });

  it("shows host status and BLE voice meters like Xiaomi page", () => {
    const src = readFileSync(t1SettingsPath, "utf8");
    expect(src).toContain("get_t1_ble_host_status");
    expect(src).toContain("get_t1_ble_voice_meter");
    expect(src).toContain("t1-ble-voice-meter");
    expect(src).toContain("音频信号");
    expect(src).toContain("虚拟声卡音量");
    expect(src).toContain("CableVolRuler");
    expect(src).toContain("host-status-row");
    expect(src).toContain("host-item-label");
    expect(src).toContain("hostItems");
    expect(src).not.toContain("host-summary");
    expect(src).toContain("ATVV 未连接");
  });

  it("shows BLE native HID probe lines for voice-key diagnosis", () => {
    const src = readFileSync(t1SettingsPath, "utf8");
    expect(src).toContain('phase === "native"');
    expect(src).toContain("[BLE原生]");
    expect(src).toContain("[USB原生]");
  });

  it("shows Xiaomi-style device info without duplicate USB/BLE fields", () => {
    const src = readFileSync(t1SettingsPath, "utf8");
    expect(src).toContain("device-info-row");
    expect(src).toContain("剩余电量");
    expect(src).toContain("BatteryLevelIcon");
    expect(src).toContain("connectionModeLabel");
    expect(src).not.toContain("VID/PID");
    expect(src).not.toContain("USB Raw Input（独立）");
    const deviceInfoBlock = src.slice(
      src.indexOf("device-info-row"),
      src.indexOf("host-card")
    );
    expect(deviceInfoBlock).not.toContain("语音快捷键");
  });

  it("splits product transport ble vs usb", () => {
    const src = readFileSync(t1SettingsPath, "utf8");
    expect(src).toContain('transport?: "ble" | "usb"');
    expect(src).toContain('"t1_ble"');
    expect(src).toContain('"t1_usb"');
  });
});
