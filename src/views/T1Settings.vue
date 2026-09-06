<script setup lang="ts">
import { onMounted, onUnmounted, computed, ref, nextTick, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useBridgeStore } from "../stores/bridge";
import { useConfigStore } from "../stores/config";
import type { DeviceConfig } from "../types";
import DeviceStatus from "../components/DeviceStatus.vue";
import BatteryLevelIcon from "../components/BatteryLevelIcon.vue";
import CableVolRuler from "../components/CableVolRuler.vue";
import T1KeyMappingStage from "../components/t1/T1KeyMappingStage.vue";
import { cableZoneForLevel } from "../utils/cableVolMeter";

type LogEntry = { id: number; time: string; text: string };

const bridge = useBridgeStore();
const configStore = useConfigStore();
const type = "t1" as const;

const device = computed(() => bridge.devices[type]);
const config = computed(() => configStore.configs[type]);

const logs = ref<LogEntry[]>([]);
const logAreaRef = ref<HTMLElement | null>(null);
let logSeq = 0;
let unlistenKey: UnlistenFn | null = null;
let unlistenBle: UnlistenFn | null = null;
let unlistenMeter: UnlistenFn | null = null;
let hostPollTimer: ReturnType<typeof setInterval> | null = null;
let devicePollTimer: ReturnType<typeof setInterval> | null = null;

/** T1 蓝牙（与 USB DeviceStatus 完全独立） */
type BleUiStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "disconnecting"
  | "error";
const bleStatus = ref<BleUiStatus>("disconnected");
const bleBusy = ref(false);
const bleName = ref("");
const bleAddress = ref("");
const bleBattery = ref<number | null>(null);
const bleMessage = ref("未连接 — 请先在系统设置配对 T1-Remote，再点蓝牙连接");
const remoteLedLit = ref(false);
const remoteLedIds = new Set<string>();
let remoteLedFlash: ReturnType<typeof setTimeout> | null = null;

function applyRemoteLed(p: {
  id?: string;
  pressed?: boolean;
  message?: string;
  phase?: string;
}) {
  const msg = p.message || "";
  const id = p.id || "";
  let pressed: boolean | undefined =
    typeof p.pressed === "boolean" ? p.pressed : undefined;
  if (pressed === undefined) {
    if (msg.includes("↓") || msg.includes("按下")) pressed = true;
    else if (msg.includes("↑") || msg.includes("抬起")) pressed = false;
  }
  if (pressed === true) {
    if (remoteLedFlash) {
      clearTimeout(remoteLedFlash);
      remoteLedFlash = null;
    }
    remoteLedIds.add(id || "anon");
    remoteLedLit.value = true;
    return;
  }
  if (pressed === false) {
    remoteLedIds.delete(id || "anon");
    if (!id) remoteLedIds.clear();
    remoteLedLit.value = remoteLedIds.size > 0;
    return;
  }
  if (id || p.phase === "key") {
    if (remoteLedFlash) clearTimeout(remoteLedFlash);
    remoteLedLit.value = true;
    remoteLedFlash = setTimeout(() => {
      remoteLedFlash = null;
      if (remoteLedIds.size === 0) remoteLedLit.value = false;
    }, 140);
  }
}

interface HostStatusItem {
  id: string;
  label: string;
  state_label: string;
  tone: string;
}
interface HostStatus {
  ble_alive: boolean;
  usb_alive: boolean;
  audio_alive: boolean;
  cable_ready: boolean;
  winuhid_ready: boolean;
  atvv_ok: boolean;
  l0_ready: boolean;
  status_text: string;
  detail: string;
  tone: string;
  items: HostStatusItem[];
}
const host = ref<HostStatus>({
  ble_alive: false,
  usb_alive: false,
  audio_alive: false,
  cable_ready: false,
  winuhid_ready: false,
  atvv_ok: false,
  l0_ready: false,
  status_text: "未启动",
  detail: "",
  tone: "error",
  items: [],
});

const l0Repairing = ref(false);
const l0RepairMsg = ref("");

type BleMeterState = "idle" | "session" | "receiving";
interface VoiceMeterSnapshot {
  bleState: BleMeterState;
  bleLevel: number;
  waveform: number[];
  cableActive: boolean;
  cableLevel: number;
  atvvOk: boolean;
}
const voiceMeter = ref<VoiceMeterSnapshot>({
  bleState: "idle",
  bleLevel: 0,
  waveform: Array(28).fill(0),
  cableActive: false,
  cableLevel: 0,
  atvvOk: false,
});

const bleStatusLabel = computed(() => {
  switch (bleStatus.value) {
    case "connected":
      return `已连接（BLE ATVV）${bleName.value ? ` · ${bleName.value}` : ""}`;
    case "connecting":
      return "蓝牙连接中…";
    case "disconnecting":
      return "正在断开蓝牙…";
    case "error":
      return bleMessage.value || "蓝牙错误";
    default:
      return bleMessage.value || "未连接";
  }
});

const bleButtonText = computed(() => {
  if (bleBusy.value) return "处理中…";
  if (bleStatus.value === "connected" || bleStatus.value === "connecting")
    return "断开蓝牙";
  return "蓝牙连接";
});

const usbConnected = computed(() => device.value.status === "Connected");
const bleConnected = computed(() => bleStatus.value === "connected");

const infoName = computed(() => {
  if (bleConnected.value && bleName.value) return bleName.value;
  if (usbConnected.value && device.value.device_name) return device.value.device_name;
  return bleName.value || device.value.device_name || "T1 Google Remote";
});

const connectionModeLabel = computed(() => {
  if (usbConnected.value && bleConnected.value) return "USB + 蓝牙 BLE";
  if (usbConnected.value) return "USB";
  if (bleConnected.value) return "蓝牙 BLE";
  return "—";
});

const usbAddress = computed(() => (device.value.device_address || "").trim());
const bleAddressValue = computed(() =>
  (bleAddress.value || config.value?.bluetooth_address || "").trim()
);

const showUsbAddress = computed(() => {
  if (usbConnected.value) {
    if (usbAddress.value && usbAddress.value === infoName.value) return false;
    return true;
  }
  return !bleConnected.value;
});
const showBleAddress = computed(() => {
  if (!bleConnected.value) return false;
  if (!usbConnected.value) return true;
  const usb = usbAddress.value.toLowerCase();
  const ble = bleAddressValue.value.toLowerCase();
  if (!usb) return true;
  if (!ble) return false;
  return usb !== ble;
});

const batteryLevel = computed(() => {
  if (bleBattery.value != null) return bleBattery.value;
  return device.value.battery_level ?? null;
});

const BLE_HOST_IDS = new Set(["cable", "winuhid", "l0", "audio", "ble", "atvv"]);
const USB_HOST_IDS = new Set(["usb", "winuhid"]);

const hostItems = computed(() => {
  const items = host.value.items;
  const bleOn = bleConnected.value || host.value.ble_alive;
  const usbOn = usbConnected.value || host.value.usb_alive;
  if (bleOn && usbOn) {
    return items.filter((i) => BLE_HOST_IDS.has(i.id) || USB_HOST_IDS.has(i.id));
  }
  if (bleOn) return items.filter((i) => BLE_HOST_IDS.has(i.id));
  if (usbOn) return items.filter((i) => USB_HOST_IDS.has(i.id));
  return items.filter((i) => i.id === "usb" || i.id === "ble");
});

function itemToneClass(tone: string): string {
  if (tone === "ok") return "ok";
  if (tone === "warn") return "warn";
  return "error";
}

const bleSignalLabel = computed(() => {
  switch (voiceMeter.value.bleState) {
    case "receiving":
      return "接收中";
    case "session":
      return "语音会话";
    default:
      return "无信号";
  }
});

const showAtvvFailLabel = computed(
  () => bleStatus.value === "connected" && !voiceMeter.value.atvvOk && !host.value.atvv_ok
);

const cableReady = computed(() => host.value.cable_ready);
const cableVolZone = computed(() => {
  if (!cableReady.value) return "idle";
  if (!voiceMeter.value.cableActive) return "idle";
  return cableZoneForLevel(voiceMeter.value.cableLevel);
});
const cableVolHint = computed(() => {
  if (!cableReady.value) return "\u00a0";
  if (!voiceMeter.value.cableActive) return "待命";
  switch (cableVolZone.value) {
    case "low":
      return "偏低";
    case "high":
      return "偏高";
    case "ok":
      return "正常";
    default:
      return "送声";
  }
});

function waveBinHeight(v: number, receiving: boolean): number {
  const clamped = Math.min(1, Math.max(0, v));
  let h = Math.pow(clamped, 0.25) * 100;
  if (receiving && h < 20) h = 20;
  return Math.max(8, h);
}

const waveAreaPath = computed(() => {
  const wf = voiceMeter.value.waveform;
  const receiving = voiceMeter.value.bleState === "receiving";
  if (!wf.length) return "M0,100 L100,100 Z";
  const top = wf
    .map((v, i) => {
      const x = wf.length === 1 ? 0 : (i / (wf.length - 1)) * 100;
      const y = 100 - waveBinHeight(v, receiving);
      return `${i === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
  return `${top} L100,100 L0,100 Z`;
});

const waveLinePoints = computed(() => {
  const wf = voiceMeter.value.waveform;
  const receiving = voiceMeter.value.bleState === "receiving";
  if (!wf.length) return "0,100 100,100";
  return wf
    .map((v, i) => {
      const x = wf.length === 1 ? 0 : (i / (wf.length - 1)) * 100;
      const y = 100 - waveBinHeight(v, receiving);
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
});

const showLogModal = ref(false);
const logText = ref("");
const logPath = ref("");
const logLoading = ref(false);
const logCopyHint = ref("");

function nowTime() {
  const d = new Date();
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
}

function prependLog(text: string) {
  logs.value.unshift({ id: ++logSeq, time: nowTime(), text });
  if (logs.value.length > 160) logs.value.length = 160;
  void nextTick(() => {
    const el = logAreaRef.value;
    if (el) el.scrollTop = 0;
  });
}

function toggleConnection() {
  if (device.value.status === "Connected") bridge.stopBridge(type);
  else bridge.startBridge(type);
}

async function toggleBleConnection() {
  if (bleBusy.value) return;
  bleBusy.value = true;
  try {
    const running = await invoke<boolean>("t1_ble_running");
    if (running || bleStatus.value === "connected" || bleStatus.value === "connecting") {
      await invoke("stop_t1_ble_bridge");
      prependLog("正在断开 T1 蓝牙…");
    } else {
      bleStatus.value = "connecting";
      bleMessage.value = "正在查找已配对的 T1-Remote…";
      prependLog("正在连接 T1 蓝牙…");
      await invoke("start_t1_ble_bridge");
    }
  } catch (e) {
    bleStatus.value = "error";
    bleMessage.value = String(e);
    prependLog(`蓝牙: ${e}`);
  } finally {
    bleBusy.value = false;
  }
}

function onBleEvent(payload: {
  phase?: string;
  message?: string;
  status?: string;
  name?: string;
  address?: string;
  level?: number;
  id?: string;
  pressed?: boolean;
}) {
  if (payload.phase !== "native" && payload.phase !== "ll") applyRemoteLed(payload);
  if (payload.message) {
    const phase = (payload.phase || "").toLowerCase();
    const tag =
      phase === "native"
        ? "[BLE原生]"
        : phase === "inject"
          ? "[BLE注入]"
          : phase === "ll"
            ? "[BLE-LL]"
            : "[BLE]";
    prependLog(`${tag} ${payload.message}`);
  }
  if (payload.name) bleName.value = payload.name;
  if (payload.address) bleAddress.value = payload.address;
  if (typeof payload.level === "number" && Number.isFinite(payload.level)) {
    bleBattery.value = Math.max(0, Math.min(100, Math.round(payload.level)));
    void bridge.refreshStatus(type);
  }
  const st = (payload.status || "").toLowerCase();
  if (st === "connected") bleStatus.value = "connected";
  else if (st === "connecting" || st === "reconnecting") bleStatus.value = "connecting";
  else if (st === "disconnecting") bleStatus.value = "disconnecting";
  else if (st === "disconnected") {
    bleStatus.value = "disconnected";
    bleBattery.value = null;
    bleMessage.value = "未连接 — 请先在系统设置配对 T1-Remote，再点蓝牙连接";
  } else if (st === "error") {
    bleStatus.value = "error";
    if (payload.message) bleMessage.value = payload.message;
  }
}

async function onKeyMappingSave(cfg: DeviceConfig) {
  const ok = await configStore.saveConfig(type, cfg);
  prependLog(ok ? "按键映射已保存" : "按键映射保存失败");
}

async function openLogs() {
  showLogModal.value = true;
  logCopyHint.value = "";
  logLoading.value = true;
  try {
    const result = await invoke<{ path: string; content: string }>("get_app_log");
    logPath.value = result.path || "";
    logText.value = result.content?.trim() ? result.content : "（暂无日志）";
  } catch (e) {
    logText.value = `读取日志失败: ${e}`;
    logPath.value = "";
  } finally {
    logLoading.value = false;
  }
}

async function copyLog() {
  try {
    await navigator.clipboard.writeText(logText.value || "");
    logCopyHint.value = "已复制";
    setTimeout(() => {
      logCopyHint.value = "";
    }, 1500);
  } catch (e) {
    logCopyHint.value = `复制失败: ${e}`;
  }
}

async function openLogFile() {
  try {
    await invoke("open_app_log");
  } catch (e) {
    logCopyHint.value = `打开失败: ${e}`;
  }
}

async function refreshHostStatus() {
  try {
    host.value = await invoke<HostStatus>("get_t1_ble_host_status");
  } catch (e) {
    console.warn("get_t1_ble_host_status failed", e);
  }
}

async function repairT1L0Filter() {
  if (l0Repairing.value) return;
  l0Repairing.value = true;
  l0RepairMsg.value = "";
  try {
    const result = await invoke<{
      ok: boolean;
      ready: boolean;
      needsReboot?: boolean;
      needs_reboot?: boolean;
      message: string;
      resultCode?: string;
      result_code?: string;
    }>("repair_t1_hid_filter");
    l0RepairMsg.value = result.message || "";
    await refreshHostStatus();
  } catch (e) {
    l0RepairMsg.value = String(e);
  } finally {
    l0Repairing.value = false;
  }
}

async function refreshVoiceMeter() {
  try {
    const snap = await invoke<{
      bleState: BleMeterState;
      bleLevel: number;
      waveform: number[];
      cableActive: boolean;
      cableLevel: number;
      atvvOk: boolean;
    }>("get_t1_ble_voice_meter");
    voiceMeter.value = {
      bleState: snap.bleState,
      bleLevel: snap.bleLevel,
      waveform: snap.waveform?.length ? snap.waveform : Array(28).fill(0),
      cableActive: snap.cableActive,
      cableLevel: snap.cableLevel,
      atvvOk: snap.atvvOk,
    };
  } catch {
    /* ignore */
  }
}

watch(
  () => device.value.status,
  (status, prev) => {
    if (status === prev) return;
    if (status === "Connected") prependLog("已连接");
    else if (status === "Connecting") prependLog("正在连接…");
    else if (status === "Disconnected") prependLog("已断开");
    else prependLog(String(status));
    void refreshHostStatus();
  }
);

onMounted(async () => {
  prependLog("日志区准备就绪");
  await Promise.all([bridge.refreshStatus(type), configStore.loadConfig(type)]);
  await Promise.all([refreshHostStatus(), refreshVoiceMeter()]);
  hostPollTimer = setInterval(() => {
    void refreshHostStatus();
  }, 2000);
  devicePollTimer = setInterval(() => {
    void bridge.refreshStatus(type);
  }, 1500);
  try {
    const running = await invoke<boolean>("t1_ble_running");
    if (running) {
      bleStatus.value = "connected";
      bleMessage.value = "蓝牙桥接运行中";
    }
  } catch {
    /* ignore */
  }
  try {
    unlistenKey = await listen<{
      id?: string;
      event?: string;
      message?: string;
      pressed?: boolean;
      phase?: string;
    }>("t1-key", (ev) => {
      const p = ev.payload;
      applyRemoteLed(p);
      if (p.message) {
        const phase = (p.phase || "").toLowerCase();
        const tag =
          phase === "native"
            ? "[USB原生]"
            : phase === "inject"
              ? "[USB注入]"
              : phase === "ll"
                ? "[USB-LL]"
                : phase === "key"
                  ? "[USB]"
                  : "[USB]";
        prependLog(`${tag} ${p.message}`);
        return;
      }
      if (p.id) prependLog(`[USB] 按键 ${p.id}${p.event ? ` (${p.event})` : ""}`);
    });
  } catch (e) {
    console.warn("listen t1-key failed", e);
    prependLog("按键日志监听失败");
  }
  try {
    unlistenBle = await listen<{
      phase?: string;
      message?: string;
      status?: string;
      name?: string;
      address?: string;
      level?: number;
      id?: string;
      pressed?: boolean;
    }>("t1-ble", (ev) => {
      onBleEvent(ev.payload);
      void refreshHostStatus();
    });
  } catch (e) {
    console.warn("listen t1-ble failed", e);
    prependLog("蓝牙日志监听失败");
  }
  try {
    unlistenMeter = await listen<VoiceMeterSnapshot>("t1-ble-voice-meter", (ev) => {
      const p = ev.payload;
      voiceMeter.value = {
        bleState: p.bleState,
        bleLevel: p.bleLevel,
        waveform: p.waveform?.length ? p.waveform : Array(28).fill(0),
        cableActive: p.cableActive,
        cableLevel: p.cableLevel,
        atvvOk: p.atvvOk,
      };
    });
  } catch (e) {
    console.warn("listen t1-ble-voice-meter failed", e);
  }
});

onUnmounted(() => {
  unlistenKey?.();
  unlistenBle?.();
  unlistenMeter?.();
  if (hostPollTimer) clearInterval(hostPollTimer);
  if (devicePollTimer) clearInterval(devicePollTimer);
  if (remoteLedFlash) clearTimeout(remoteLedFlash);
});
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h2>T1 遥控器</h2>
      <div class="header-actions">
        <DeviceStatus
          :status="device.status"
          :loading="bridge.loading[type]"
          @toggle="toggleConnection"
        />
        <div class="ble-status">
          <span
            :class="[
              'status-indicator',
              bleStatus === 'connected'
                ? 'connected'
                : bleStatus === 'connecting' || bleStatus === 'disconnecting'
                  ? 'connecting'
                  : bleStatus === 'error'
                    ? 'error'
                    : 'disconnected',
            ]"
          >
            <span class="dot"></span>
            {{ bleStatusLabel }}
          </span>
          <button
            :class="[
              'btn',
              bleStatus === 'connected' || bleStatus === 'connecting'
                ? 'btn-danger'
                : 'btn-primary',
            ]"
            type="button"
            :disabled="bleBusy || bleStatus === 'disconnecting'"
            @click="toggleBleConnection"
          >
            {{ bleButtonText }}
          </button>
        </div>
      </div>
    </header>

    <div class="overview-row">
      <div class="overview-left">
        <div class="device-info-row">
          <div class="device-info-col">
            <div class="info-line">
              <span class="info-label">设备名称</span>
              <span class="info-value">{{ infoName }}</span>
            </div>
            <div v-if="showUsbAddress" class="info-line">
              <span class="info-label">设备地址</span>
              <span class="info-value">{{ usbAddress || "—" }}</span>
            </div>
            <div v-if="showBleAddress" class="info-line">
              <span class="info-label">蓝牙地址</span>
              <span class="info-value">{{ bleAddressValue || "—" }}</span>
            </div>
          </div>
          <div class="device-info-col">
            <div class="info-line">
              <span class="info-label">剩余电量</span>
              <span class="info-value info-value-battery">
                <BatteryLevelIcon :level="batteryLevel" />
                {{ batteryLevel != null ? batteryLevel + "%" : "—" }}
              </span>
            </div>
            <div class="info-line">
              <span class="info-label">连接方式</span>
              <span class="info-value">{{ connectionModeLabel }}</span>
            </div>
          </div>
          <div
            class="info-item info-item-audio"
            :class="{
              'is-session': voiceMeter.bleState === 'session',
              'is-receiving': voiceMeter.bleState === 'receiving',
            }"
            title="T1 BLE ATVV 解码后的 PCM"
          >
            <div class="audio-label-row">
              <span class="info-label">音频信号</span>
              <span v-if="showAtvvFailLabel" class="audio-atvv-fail">ATVV 未连接</span>
              <span v-else-if="voiceMeter.bleState !== 'idle'" class="audio-state">{{
                bleSignalLabel
              }}</span>
            </div>
            <div class="ble-wave" aria-hidden="true">
              <svg class="ble-wave-svg" viewBox="0 0 100 100" preserveAspectRatio="none">
                <path class="ble-wave-fill" :d="waveAreaPath" />
                <polyline class="ble-wave-line" :points="waveLinePoints" />
              </svg>
            </div>
          </div>
          <div
            class="info-item info-item-cable-vol"
            :class="[
              `cable-zone-${cableVolZone}`,
              { 'is-active': cableReady && voiceMeter.cableActive },
            ]"
            title="经 BLE 送往 VB-CABLE 的实时电平"
          >
            <div class="audio-label-row cable-vol-label-row">
              <span v-if="cableReady" class="info-label">虚拟声卡音量</span>
              <span v-else class="cable-vol-fail">虚拟声卡未就绪</span>
              <span class="cable-vol-state">{{ cableVolHint }}</span>
            </div>
            <CableVolRuler
              :level="voiceMeter.cableLevel"
              :disabled="!cableReady"
              :active="cableReady && voiceMeter.cableActive"
            />
          </div>
        </div>

        <section class="card host-card">
          <div class="host-status-row" role="list" aria-label="运行状态">
            <div
              v-for="item in hostItems"
              :key="item.id"
              class="host-status-item"
              role="listitem"
            >
              <span
                class="host-dot"
                :class="itemToneClass(item.tone)"
                aria-hidden="true"
              />
              <span class="host-item-label">{{ item.label }}</span>
              <span class="host-item-state" :class="itemToneClass(item.tone)">
                {{ item.state_label }}
              </span>
            </div>
          </div>
          <div class="host-l0-row">
            <button
              class="btn btn-secondary host-l0-btn"
              type="button"
              :disabled="l0Repairing"
              title="仅禁用 T1 蓝牙 Consumer Control（AC Search），不碰键盘/鼠标/其它电脑外设；无需改 BIOS"
              @click="repairT1L0Filter"
            >
              {{ l0Repairing ? "修复中…" : "自动修复 Search 剥离" }}
            </button>
            <span class="host-l0-hint">
              禁用 T1 的 Consumer HID（切断 AC Search）；需 UAC，不改 BIOS / 不装自签驱动
            </span>
          </div>
          <p v-if="l0RepairMsg" class="host-l0-msg">{{ l0RepairMsg }}</p>
          <p v-if="host.detail" class="host-detail">{{ host.detail }}</p>
        </section>
      </div>

      <aside class="log-aside">
        <section class="card log-card">
          <div class="log-card-head">
            <p class="card-text">状态日志</p>
            <button class="btn btn-tiny btn-secondary" type="button" @click="openLogs">
              日志
            </button>
          </div>
          <div ref="logAreaRef" class="log-area">
            <p v-for="entry in logs" :key="entry.id" class="log-entry">
              <span class="log-time">{{ entry.time }}</span>
              <span class="log-text">{{ entry.text }}</span>
            </p>
          </div>
        </section>
      </aside>
    </div>

    <div class="page-body">
      <section class="card mapping-card" v-if="config">
        <div class="mapping-heading">
          <h3>按键映射</h3>
        </div>
        <T1KeyMappingStage
          :config="config"
          :hardware-lit="remoteLedLit"
          @save="onKeyMappingSave"
        />
      </section>
    </div>

    <Teleport to="body">
      <div v-if="showLogModal" class="log-modal-backdrop" @click.self="showLogModal = false">
        <div class="log-modal" role="dialog" aria-modal="true">
          <div class="log-modal-head">
            <h3>应用日志</h3>
            <div class="log-modal-actions">
              <span v-if="logCopyHint" class="log-hint">{{ logCopyHint }}</span>
              <button class="btn btn-secondary" type="button" @click="copyLog">复制</button>
              <button class="btn btn-secondary" type="button" @click="openLogFile">打开文件</button>
              <button class="btn btn-secondary" type="button" @click="showLogModal = false">关闭</button>
            </div>
          </div>
          <p v-if="logPath" class="log-path">{{ logPath }}</p>
          <pre class="log-modal-body">{{ logLoading ? "读取中…" : logText }}</pre>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.page {
  width: 100%;
  max-width: none;
  box-sizing: border-box;
}
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
  gap: 12px;
  flex-wrap: wrap;
}
.header-actions {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
}
.ble-status {
  display: flex;
  align-items: center;
  gap: 12px;
}
.status-indicator {
  font-size: 13px;
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.status-indicator .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.status-indicator.connected {
  color: var(--success);
}
.status-indicator.connected .dot {
  background: var(--success);
}
.status-indicator.connecting {
  color: var(--warning, #d97706);
}
.status-indicator.connecting .dot {
  background: var(--warning, #d97706);
}
.status-indicator.error {
  color: var(--danger, #dc2626);
}
.status-indicator.error .dot {
  background: var(--danger, #dc2626);
}
.status-indicator.disconnected {
  color: var(--text-muted, #6b7280);
}
.status-indicator.disconnected .dot {
  background: var(--text-muted, #9ca3af);
}
.page-header h2 {
  font-size: 20px;
  font-weight: 600;
}

.overview-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  gap: 12px;
  align-items: stretch;
  margin-bottom: 16px;
}
.overview-left {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
.log-aside {
  position: relative;
  min-height: 0;
}
.log-card {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 8px;
  overflow: hidden;
  box-sizing: border-box;
}
.log-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
  margin-bottom: 4px;
}
.card-text {
  margin: 0;
  font-size: 12px;
  color: var(--text);
}
.log-hint {
  margin: 0 0 6px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--text-muted, #8b93a1);
}
.log-area {
  flex: 1;
  min-height: 0;
  overflow: auto;
  font-size: 12px;
  line-height: 1.45;
  padding: 4px 2px;
}
.log-entry {
  margin: 0 0 4px;
  display: flex;
  gap: 6px;
}
.log-time {
  flex-shrink: 0;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
.log-text {
  color: var(--text);
  word-break: break-word;
}

@media (max-width: 840px) {
  .overview-row {
    grid-template-columns: 1fr;
  }
  .log-aside {
    position: static;
    height: 180px;
  }
  .log-card {
    position: relative;
    inset: auto;
    height: 100%;
  }
}

.page-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 16px 18px;
}
.card h3 {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 12px;
}

.mapping-card {
  padding-bottom: 8px;
}
.mapping-heading h3 {
  margin-bottom: 14px;
}

.device-info-row {
  display: flex;
  gap: 16px;
  margin-bottom: 0;
  padding: 12px 14px;
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  align-items: stretch;
}
.device-info-col {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 8px;
  min-width: 0;
}
.device-info-col:first-child {
  flex: 1.6 1 auto;
  min-width: 13.5em;
}
.device-info-col:nth-child(2) {
  flex: 0 1 auto;
  min-width: 8.5em;
}
.info-line {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}
.info-line .info-label {
  flex-shrink: 0;
}
.info-line .info-value {
  font-size: 12px;
  font-weight: 400;
  color: var(--text, #1e293b);
  white-space: nowrap;
}
.info-value-battery {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.info-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.info-label {
  font-size: 12px;
  color: var(--text-secondary);
}
.info-value {
  font-size: 14px;
  font-weight: 500;
}
@media (max-width: 720px) {
  .device-info-row {
    flex-direction: column;
  }
  .info-item-audio,
  .info-item-cable-vol {
    width: 100%;
  }
}

.btn-tiny {
  padding: 2px 8px;
  font-size: 12px;
}

.log-modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 80;
  padding: 24px;
}
.log-modal {
  width: min(920px, 100%);
  max-height: min(80vh, 720px);
  background: var(--card-bg, #fff);
  border-radius: 10px;
  border: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.log-modal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
}
.log-modal-head h3 {
  margin: 0;
  font-size: 15px;
}
.log-modal-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.log-hint {
  font-size: 12px;
  color: var(--text-secondary);
}
.log-path {
  margin: 0;
  padding: 6px 16px;
  font-size: 11px;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border);
  word-break: break-all;
}
.log-modal-body {
  margin: 0;
  padding: 12px 16px;
  overflow: auto;
  flex: 1;
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
}

.host-card {
  padding: 12px 14px;
}
.host-status-row {
  display: flex;
  flex-wrap: nowrap;
  gap: 8px;
  align-items: stretch;
  margin: 0;
  overflow-x: auto;
}
.host-l0-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
}
.host-l0-btn {
  flex-shrink: 0;
}
.host-l0-hint {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.35;
}
.host-l0-msg {
  margin: 8px 0 0;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.4;
}
.host-status-item {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 0 1 auto;
  width: max-content;
  min-width: 0;
  padding: 10px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #fff;
}
.host-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  background: #94a3b8;
}
.host-dot.ok {
  background: var(--success, #22c55e);
}
.host-dot.warn {
  background: var(--warning, #f59e0b);
}
.host-dot.error {
  background: var(--danger, #ef4444);
}
.host-item-label {
  font-size: 13px;
  font-weight: 400;
  color: var(--text);
  white-space: nowrap;
  flex-shrink: 0;
}
.host-item-state {
  margin-left: 4px;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  white-space: nowrap;
}
.host-item-state.ok {
  color: #15803d;
}
.host-item-state.warn {
  color: #b45309;
}
.host-item-state.error {
  color: #b91c1c;
}
.host-detail {
  margin: 8px 0 0;
  font-size: 13px;
  color: #555;
  line-height: 1.5;
}

.info-item-audio,
.info-item-cable-vol {
  gap: 3px;
  flex: 1.1 1 0;
  min-width: 120px;
}
.audio-label-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 18px;
}
.audio-state {
  font-size: 12px;
  color: var(--success, #16a34a);
}
.audio-atvv-fail,
.cable-vol-fail {
  font-size: 12px;
  color: var(--danger, #dc2626);
}
.ble-wave {
  height: 28px;
  border-radius: 4px;
  background: #f1f5f9;
  overflow: hidden;
}
.ble-wave-svg {
  width: 100%;
  height: 100%;
  display: block;
}
.ble-wave-fill {
  fill: rgba(34, 197, 94, 0.25);
}
.ble-wave-line {
  fill: none;
  stroke: #16a34a;
  stroke-width: 1.5;
}
.info-item-audio.is-receiving .ble-wave {
  background: #ecfdf5;
}
.cable-vol-label-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
}
.cable-vol-state {
  font-size: 12px;
  color: var(--text-secondary);
}
</style>
