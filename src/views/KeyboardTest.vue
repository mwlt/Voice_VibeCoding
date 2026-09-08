<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  KEYBOARD_MAIN,
  KEYBOARD_MEDIA,
  KEYBOARD_NAV,
  KEYBOARD_NUMPAD,
  KEYBOARD_NUMPAD_MEDIA,
  formatKeyLabel,
  keyTone,
  resolveEventCode,
  type KbKey,
} from "../utils/keyboardLayout";
import { T1_DEFAULT_LABELS, type T1ButtonId } from "../utils/t1Keys";

type KbLog = {
  kind: "keyboard";
  id: number;
  time: string;
  phase: "↓" | "↑";
  key: string;
  code: string;
  keyCode: number;
  location: number;
  mods: string;
  repeat: boolean;
};

type RemoteLog = {
  kind: "remote";
  id: number;
  time: string;
  brand: "xiaomi" | "t1_usb" | "t1_ble";
  phase: "↓" | "↑" | "·";
  buttonId: string;
  label: string;
  detail?: string;
};

type LogEntry = KbLog | RemoteLog;

/** 图例状态优先级：Pressed > Stuck > Tested > Didn't register > Untested */
type KeyVisualState = "untested" | "pressed" | "tested" | "stuck" | "missed";

const XIAOMI_LABELS: Record<string, string> = {
  power: "电源",
  volume_up: "音量+",
  volume_down: "音量-",
  mute: "静音",
  up: "上",
  down: "下",
  left: "左",
  right: "右",
  dpad_up: "上",
  dpad_down: "下",
  dpad_left: "左",
  dpad_right: "右",
  ok: "确定",
  home: "主页",
  back: "返回",
  menu: "菜单",
  mic: "语音",
  voice: "语音",
};

const MAX_RECORDS = 500;
const STUCK_MS = 2000;

const pressed = reactive<Record<string, boolean>>({});
const tested = reactive<Record<string, boolean>>({});
const stuck = reactive<Record<string, boolean>>({});
const missed = reactive<Record<string, boolean>>({});

const records = ref<LogEntry[]>([]);
const copyHint = ref("");
const listening = ref(true);
const watchXiaomi = ref(false);
const watchT1Usb = ref(false);
const watchT1Ble = ref(false);
const logEndRef = ref<HTMLElement | null>(null);
const pageRef = ref<HTMLElement | null>(null);

let seq = 0;
const stuckTimers = new Map<string, ReturnType<typeof setTimeout>>();
let unlistenXiaomi: UnlistenFn | null = null;
let unlistenT1Usb: UnlistenFn | null = null;
let unlistenT1Ble: UnlistenFn | null = null;

function formatEntry(r: LogEntry): string {
  if (r.kind === "keyboard") {
    const mods = r.mods ? ` mods=${r.mods}` : "";
    const rep = r.repeat ? " repeat" : "";
    return `${r.time}  [键盘] ${r.phase} key=${r.key} code=${r.code} keyCode=${r.keyCode} loc=${r.location}${mods}${rep}`;
  }
  const brand =
    r.brand === "xiaomi" ? "小米" : r.brand === "t1_usb" ? "T1-USB" : "T1-蓝牙";
  const detail = r.detail ? `  ${r.detail}` : "";
  return `${r.time}  [${brand}] ${r.phase} ${r.label} (${r.buttonId})${detail}`;
}

const recordText = computed(() => records.value.map(formatEntry).join("\n"));

const lastDown = computed(() => {
  for (let i = records.value.length - 1; i >= 0; i--) {
    const r = records.value[i];
    if (r.kind === "keyboard" && r.phase === "↓") return r;
  }
  return null;
});

function pad2(n: number) {
  return n.toString().padStart(2, "0");
}

function nowStamp() {
  const d = new Date();
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}.${d
    .getMilliseconds()
    .toString()
    .padStart(3, "0")}`;
}

function modsOf(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Win");
  return parts.join("+");
}

function clearStuckTimer(code: string) {
  const t = stuckTimers.get(code);
  if (t) {
    clearTimeout(t);
    stuckTimers.delete(code);
  }
}

function armStuckTimer(code: string) {
  clearStuckTimer(code);
  stuckTimers.set(
    code,
    setTimeout(() => {
      if (pressed[code]) {
        stuck[code] = true;
      }
      stuckTimers.delete(code);
    }, STUCK_MS)
  );
}

function visualState(code: string): KeyVisualState {
  if (pressed[code]) return "pressed";
  if (stuck[code]) return "stuck";
  if (tested[code]) return "tested";
  if (missed[code]) return "missed";
  return "untested";
}

function trimRecords() {
  if (records.value.length > MAX_RECORDS) {
    records.value.splice(0, records.value.length - MAX_RECORDS);
  }
  nextTick(() => {
    logEndRef.value?.scrollIntoView({ block: "end" });
  });
}

function pushKeyboard(e: KeyboardEvent, phase: "↓" | "↑") {
  seq += 1;
  records.value.push({
    kind: "keyboard",
    id: seq,
    time: nowStamp(),
    phase,
    key: formatKeyLabel(e),
    code: resolveEventCode(e),
    keyCode: e.keyCode || e.which || 0,
    location: e.location,
    mods: modsOf(e),
    repeat: e.repeat,
  });
  trimRecords();
}

function pushRemote(
  brand: "xiaomi" | "t1_usb" | "t1_ble",
  phase: "↓" | "↑" | "·",
  buttonId: string,
  label: string,
  detail?: string
) {
  seq += 1;
  records.value.push({
    kind: "remote",
    id: seq,
    time: nowStamp(),
    brand,
    phase,
    buttonId,
    label,
    detail,
  });
  trimRecords();
}

function xiaomiLabel(id: string, fallback?: string): string {
  return fallback || XIAOMI_LABELS[id] || id;
}

function t1Label(id: string): string {
  return T1_DEFAULT_LABELS[id as T1ButtonId] || id;
}

function onKeyDown(e: KeyboardEvent) {
  if (!listening.value) return;
  if (!(e.altKey && (e.key === "F4" || e.code === "F4"))) {
    e.preventDefault();
  }
  const code = resolveEventCode(e);
  if (!e.repeat) {
    pressed[code] = true;
    delete missed[code];
    armStuckTimer(code);
  }
  pushKeyboard(e, "↓");
}

function onKeyUp(e: KeyboardEvent) {
  if (!listening.value) return;
  e.preventDefault();
  const code = resolveEventCode(e);
  delete pressed[code];
  clearStuckTimer(code);
  delete stuck[code];
  tested[code] = true;
  delete missed[code];
  pushKeyboard(e, "↑");
}

function keyClass(key: KbKey, extra = "") {
  if (key.gap || !key.code) return ["kb-key", "kb-key--gap", extra].filter(Boolean).join(" ");
  const tone = keyTone(key);
  const state = visualState(key.code);
  return [
    "kb-key",
    tone === "mod" ? "kb-key--mod" : "kb-key--alpha",
    `is-${state}`,
    extra,
    (key.w ?? 1) >= 2 ? "kb-key--span2w" : "",
    (key.h ?? 1) >= 2 ? "kb-key--span2h" : "",
  ]
    .filter(Boolean)
    .join(" ");
}

function markUntestedAsMissed() {
  const allCodes = new Set<string>();
  for (const row of KEYBOARD_MAIN) for (const k of row) if (k.code) allCodes.add(k.code);
  for (const row of KEYBOARD_NAV) for (const k of row) if (k.code) allCodes.add(k.code);
  for (const k of KEYBOARD_NUMPAD) if (k.code) allCodes.add(k.code);
  for (const k of KEYBOARD_NUMPAD_MEDIA) if (k.code) allCodes.add(k.code);
  for (const k of KEYBOARD_MEDIA) if (k.code) allCodes.add(k.code);
  for (const code of allCodes) {
    if (!tested[code] && !pressed[code] && !stuck[code]) {
      missed[code] = true;
    }
  }
}

function clearLog() {
  records.value = [];
  for (const k of Object.keys(pressed)) delete pressed[k];
  for (const k of Object.keys(tested)) delete tested[k];
  for (const k of Object.keys(stuck)) delete stuck[k];
  for (const k of Object.keys(missed)) delete missed[k];
  for (const t of stuckTimers.values()) clearTimeout(t);
  stuckTimers.clear();
  copyHint.value = "";
}

async function copyLog() {
  const text = recordText.value;
  if (!text) {
    copyHint.value = "暂无记录";
    return;
  }
  try {
    await navigator.clipboard.writeText(text);
    copyHint.value = "已复制";
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand("copy");
      copyHint.value = "已复制";
    } catch {
      copyHint.value = "复制失败";
    }
    document.body.removeChild(ta);
  }
  setTimeout(() => {
    copyHint.value = "";
  }, 1600);
}

function focusPage() {
  pageRef.value?.focus();
}

async function startXiaomiListen() {
  if (unlistenXiaomi) return;
  try {
    unlistenXiaomi = await listen<{
      buttonId?: string;
      label?: string;
      message?: string;
      phase?: string;
    }>("xiaomi-key", (event) => {
      if (!watchXiaomi.value) return;
      const p = event.payload;
      if (p.message && !p.buttonId) {
        pushRemote("xiaomi", "·", "msg", "消息", p.message);
        return;
      }
      const id = p.buttonId || "unknown";
      const phase: "↓" | "↑" = p.phase === "up" ? "↑" : "↓";
      pushRemote("xiaomi", phase, id, xiaomiLabel(id, p.label), p.message);
    });
  } catch (e) {
    console.warn("listen xiaomi-key failed:", e);
  }
}

async function stopXiaomiListen() {
  if (unlistenXiaomi) {
    unlistenXiaomi();
    unlistenXiaomi = null;
  }
}

type T1Payload = {
  id?: string;
  event?: string;
  message?: string;
  pressed?: boolean;
  phase?: string;
  status?: string;
};

function parseT1Phase(p: T1Payload): "↓" | "↑" | "·" {
  if (typeof p.pressed === "boolean") return p.pressed ? "↓" : "↑";
  const msg = p.message || "";
  if (msg.includes("↓") || msg.includes("按下")) return "↓";
  if (msg.includes("↑") || msg.includes("抬起")) return "↑";
  return "·";
}

function handleT1Payload(brand: "t1_usb" | "t1_ble", enabled: boolean, p: T1Payload) {
  if (!enabled) return;
  if (!p.id && !p.message) return;
  const st = (p.status || "").toLowerCase();
  if (!p.id && st && !p.message) return;
  const phase = parseT1Phase(p);
  if (p.id) {
    pushRemote(
      brand,
      phase,
      p.id,
      t1Label(p.id),
      p.message || (p.event ? `event=${p.event}` : undefined)
    );
    return;
  }
  if (p.message) {
    pushRemote(brand, phase, "msg", "消息", p.message);
  }
}

async function startT1UsbListen() {
  if (unlistenT1Usb) return;
  try {
    unlistenT1Usb = await listen<T1Payload>("t1-key", (ev) =>
      handleT1Payload("t1_usb", watchT1Usb.value, ev.payload || {})
    );
  } catch (e) {
    console.warn("listen t1-key failed:", e);
  }
}

async function stopT1UsbListen() {
  if (unlistenT1Usb) {
    unlistenT1Usb();
    unlistenT1Usb = null;
  }
}

async function startT1BleListen() {
  if (unlistenT1Ble) return;
  try {
    unlistenT1Ble = await listen<T1Payload>("t1-ble", (ev) =>
      handleT1Payload("t1_ble", watchT1Ble.value, ev.payload || {})
    );
  } catch (e) {
    console.warn("listen t1-ble failed:", e);
  }
}

async function stopT1BleListen() {
  if (unlistenT1Ble) {
    unlistenT1Ble();
    unlistenT1Ble = null;
  }
}

watch(watchXiaomi, (on) => {
  if (on) void startXiaomiListen();
  else void stopXiaomiListen();
});

watch(watchT1Usb, (on) => {
  if (on) void startT1UsbListen();
  else void stopT1UsbListen();
});

watch(watchT1Ble, (on) => {
  if (on) void startT1BleListen();
  else void stopT1BleListen();
});

onMounted(() => {
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onKeyUp, true);
  focusPage();
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeyDown, true);
  window.removeEventListener("keyup", onKeyUp, true);
  void stopXiaomiListen();
  void stopT1UsbListen();
  void stopT1BleListen();
  for (const t of stuckTimers.values()) clearTimeout(t);
  stuckTimers.clear();
});
</script>

<template>
  <div ref="pageRef" class="page" tabindex="0" @click="focusPage">
    <header class="page-header">
      <div class="header-left">
        <h2>⌨️ 键盘测试</h2>
        <span class="header-hint"
          >以「监听中」键盘高亮为准（与记事本相同）。勾选 T1蓝牙 只是遥控诊断日志，不代表已注入系统。</span
        >
      </div>
      <div class="header-actions">
        <label class="listen-toggle">
          <input v-model="watchXiaomi" type="checkbox" />
          <span>小米</span>
        </label>
        <label class="listen-toggle">
          <input v-model="watchT1Usb" type="checkbox" />
          <span>T1 USB</span>
        </label>
        <label class="listen-toggle">
          <input v-model="watchT1Ble" type="checkbox" />
          <span>T1 蓝牙</span>
        </label>
        <label class="listen-toggle">
          <input v-model="listening" type="checkbox" />
          <span>{{ listening ? "监听中" : "已暂停" }}</span>
        </label>
        <button type="button" class="btn btn-secondary" @click="markUntestedAsMissed">
          未测→未登记
        </button>
        <button type="button" class="btn btn-secondary" @click="clearLog">清空</button>
        <button type="button" class="btn btn-primary" @click="copyLog">复制记录</button>
        <span v-if="copyHint" class="copy-hint">{{ copyHint }}</span>
      </div>
    </header>

    <div class="page-body">
      <section class="card last-card">
        <div class="last-row">
          <span class="last-label">最近按下</span>
          <template v-if="lastDown">
            <span class="last-key">{{ lastDown.key }}</span>
            <span class="last-meta"
              >code={{ lastDown.code }} · keyCode={{ lastDown.keyCode
              }}{{ lastDown.mods ? ` · ${lastDown.mods}` : "" }}</span
            >
          </template>
          <span v-else class="last-empty">等待按键…（点击本页任意处确保焦点）</span>
        </div>
      </section>

      <section class="card kb-card">
        <div class="kb-legend" aria-label="按键状态图例">
          <span class="leg"><i class="sw sw-untested" />Untested 未测</span>
          <span class="leg"><i class="sw sw-pressed" />Pressed 按下中</span>
          <span class="leg"><i class="sw sw-tested" />Tested 已测过</span>
          <span class="leg"><i class="sw sw-stuck" />Stuck 卡住了</span>
          <span class="leg"><i class="sw sw-missed" />Didn't register 没反应</span>
        </div>

        <div class="kb-board" aria-hidden="true">
          <div class="kb-main">
            <div v-for="(row, ri) in KEYBOARD_MAIN" :key="'m' + ri" class="kb-row">
              <div
                v-for="(key, ki) in row"
                :key="'m' + ri + '-' + ki + key.code"
                :class="keyClass(key)"
                :style="{ flexGrow: key.w ?? 1, flexBasis: `${(key.w ?? 1) * 38}px` }"
              >
                <span v-if="!key.gap" class="kb-label">{{ key.label }}</span>
              </div>
            </div>
          </div>
          <div class="kb-nav">
            <div v-for="(row, ri) in KEYBOARD_NAV" :key="'n' + ri" class="kb-row">
              <div
                v-for="(key, ki) in row"
                :key="'n' + ri + '-' + ki + key.code"
                :class="keyClass(key)"
                :style="{ flexGrow: 1, flexBasis: '38px' }"
              >
                <span v-if="!key.gap" class="kb-label">{{ key.label }}</span>
              </div>
            </div>
          </div>
          <div class="kb-numpad">
            <div class="kb-numpad-media">
              <div
                v-for="key in KEYBOARD_NUMPAD_MEDIA"
                :key="'nm-' + key.code"
                :class="keyClass(key, 'kb-key--media')"
                :title="key.label"
              >
                <!-- Mute -->
                <svg
                  v-if="key.icon === 'mute'"
                  class="kb-ico"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M11 5 6 9H3v6h3l5 4V5z" />
                  <path d="m23 9-6 6M17 9l6 6" />
                </svg>
                <!-- Vol- -->
                <svg
                  v-else-if="key.icon === 'vol-down'"
                  class="kb-ico"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M11 5 6 9H3v6h3l5 4V5z" />
                  <path d="M15.5 12h5" />
                </svg>
                <!-- Vol+ -->
                <svg
                  v-else-if="key.icon === 'vol-up'"
                  class="kb-ico"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M11 5 6 9H3v6h3l5 4V5z" />
                  <path d="M16 9v6M13 12h6" />
                </svg>
                <!-- Calculator -->
                <svg
                  v-else-if="key.icon === 'calc'"
                  class="kb-ico"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <rect x="4" y="2" width="16" height="20" rx="2" />
                  <path d="M8 6h8M8 10h2M12 10h2M16 10h2M8 14h2M12 14h2M16 14h2M8 18h2M12 18h2M16 18h2" />
                </svg>
              </div>
            </div>
            <div class="kb-numpad-grid">
              <div
                v-for="key in KEYBOARD_NUMPAD"
                :key="'np-' + key.code"
                :class="keyClass(key)"
              >
                <span class="kb-label">{{ key.label }}</span>
              </div>
            </div>
          </div>
        </div>

        <div class="kb-media">
          <span class="media-title">其它媒体 / 浏览器键</span>
          <div class="kb-row media-row">
            <div
              v-for="key in KEYBOARD_MEDIA"
              :key="key.code"
              :class="keyClass(key)"
              :style="{ flexGrow: 1, flexBasis: '72px' }"
            >
              <span class="kb-label">{{ key.label }}</span>
            </div>
          </div>
        </div>
      </section>

      <section class="card log-card">
        <div class="log-head">
          <h3>按键记录</h3>
          <span class="log-count">{{ records.length }} 条</span>
        </div>
        <div class="log-box" role="log" aria-live="polite">
          <div v-if="!records.length" class="log-empty">
            尚无记录。勾选小米 / T1 USB / T1 蓝牙后，遥控与键盘事件会按时间依次记下。
          </div>
          <div
            v-for="r in records"
            :key="r.id"
            :class="['log-line', r.kind === 'remote' ? 'log-remote' : 'log-kb']"
          >
            <span class="log-time">{{ r.time }}</span>
            <template v-if="r.kind === 'keyboard'">
              <span class="log-src src-kb">键盘</span>
              <span :class="['log-phase', r.phase === '↓' ? 'down' : 'up']">{{ r.phase }}</span>
              <span class="log-key">{{ r.key }}</span>
              <span class="log-code">{{ r.code }}</span>
              <span class="log-vk">VK {{ r.keyCode }}</span>
              <span v-if="r.mods" class="log-mods">{{ r.mods }}</span>
              <span v-if="r.repeat" class="log-rep">repeat</span>
            </template>
            <template v-else>
              <span
                :class="[
                  'log-src',
                  r.brand === 'xiaomi'
                    ? 'src-xm'
                    : r.brand === 't1_usb'
                      ? 'src-t1-usb'
                      : 'src-t1-ble',
                ]"
                >{{
                  r.brand === "xiaomi" ? "小米" : r.brand === "t1_usb" ? "T1-USB" : "T1-蓝牙"
                }}</span
              >
              <span
                :class="[
                  'log-phase',
                  r.phase === '↓' ? 'down' : r.phase === '↑' ? 'up' : 'note',
                ]"
                >{{ r.phase }}</span
              >
              <span class="log-key">{{ r.label }}</span>
              <span class="log-code">{{ r.buttonId }}</span>
              <span v-if="r.detail" class="log-detail">{{ r.detail }}</span>
            </template>
          </div>
          <div ref="logEndRef" />
        </div>
        <textarea
          class="log-raw"
          readonly
          :value="recordText"
          aria-label="可复制的原始记录"
          placeholder="原始文本会同步到这里，也可直接全选复制"
        />
      </section>
    </div>
  </div>
</template>

<style scoped>
.page {
  width: 100%;
  max-width: none;
  box-sizing: border-box;
  outline: none;
}
.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 16px;
  flex-wrap: wrap;
}
.page-header h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  white-space: nowrap;
}
.header-left {
  display: flex;
  align-items: baseline;
  gap: 12px;
  min-width: 0;
  flex: 1;
}
.header-hint {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.4;
}
.header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  flex-wrap: wrap;
}
.listen-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
  user-select: none;
  margin-right: 4px;
}
.copy-hint {
  font-size: 12px;
  color: var(--success, #22c55e);
}
.page-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 16px 18px;
}
.last-card {
  padding: 12px 18px;
}
.last-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  min-height: 28px;
}
.last-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.last-key {
  font-size: 18px;
  font-weight: 700;
  color: var(--primary, #1a73e8);
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
}
.last-meta {
  font-size: 12px;
  color: var(--text-secondary);
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
}
.last-empty {
  font-size: 13px;
  color: var(--text-secondary);
}

.kb-card {
  overflow-x: auto;
  background: #f9fafb;
  border-color: #e5e7eb;
}
.kb-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 14px 18px;
  align-items: center;
  margin-bottom: 14px;
  font-size: 12px;
  color: #6b7280;
}
.leg {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.sw {
  display: inline-block;
  width: 14px;
  height: 14px;
  border-radius: 4px;
  box-sizing: border-box;
  flex-shrink: 0;
}
.sw-untested {
  background: #fff;
  border: 1px solid #e5e7eb;
}
.sw-pressed {
  background: #3b82f6;
  border: 1px solid #3b82f6;
}
.sw-tested {
  background: #dcfce7;
  border: 1px solid #86efac;
}
.sw-stuck {
  background: #fef9c3;
  border: 1px solid #fde047;
}
.sw-missed {
  background: #fee2e2;
  border: 1px solid #fca5a5;
}

.kb-board {
  display: flex;
  gap: 14px;
  align-items: flex-start;
  min-width: 1040px;
}
.kb-main {
  flex: 1;
  min-width: 0;
}
.kb-nav {
  width: 132px;
  flex-shrink: 0;
}
.kb-numpad {
  width: 176px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.kb-numpad-media {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 6px;
}
.kb-numpad-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  grid-auto-rows: 38px;
  grid-auto-flow: dense;
  gap: 6px;
}
.kb-key--span2w {
  grid-column: span 2;
}
.kb-key--span2h {
  grid-row: span 2;
  height: auto !important;
}
.kb-key--media {
  height: 38px;
  padding: 0;
}
.kb-row {
  display: flex;
  gap: 6px;
  margin-bottom: 6px;
}

/* —— 3D 键帽：白 / 浅灰蓝两种底色 + 厚度阴影 —— */
.kb-key {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 38px;
  min-width: 0;
  padding: 0 4px;
  border-radius: 10px;
  color: #374151;
  font-size: 11px;
  font-weight: 600;
  line-height: 1.1;
  text-align: center;
  user-select: none;
  box-sizing: border-box;
  transition: background 0.1s ease, color 0.1s ease, border-color 0.1s ease,
    box-shadow 0.1s ease, transform 0.08s ease;
}
.kb-key--alpha {
  background: #ffffff;
  border: 1px solid #e5e7eb;
  box-shadow: 0 3px 0 #d1d5db, 0 4px 8px rgba(15, 23, 42, 0.06);
}
.kb-key--mod {
  background: #ebeef5;
  border: 1px solid #d5dae6;
  box-shadow: 0 3px 0 #c5cbd8, 0 4px 8px rgba(15, 23, 42, 0.06);
}
.kb-label {
  white-space: pre-line;
  pointer-events: none;
}
.kb-ico {
  width: 16px;
  height: 16px;
  display: block;
  pointer-events: none;
}
.kb-key--gap {
  visibility: hidden;
  border: none !important;
  box-shadow: none !important;
  background: transparent !important;
}

/* Pressed：实心蓝，按下沉 */
.kb-key.is-pressed {
  background: #3b82f6 !important;
  border-color: #2563eb !important;
  color: #fff !important;
  box-shadow: 0 1px 0 #1d4ed8, 0 1px 3px rgba(37, 99, 235, 0.35) !important;
  transform: translateY(2px);
}

/* Tested：浅绿底 + 绿边 */
.kb-key.is-tested:not(.is-pressed) {
  background: #dcfce7 !important;
  border-color: #86efac !important;
  box-shadow: 0 2px 0 #86efac, 0 3px 6px rgba(16, 185, 129, 0.12) !important;
  color: #166534;
}

/* Stuck：浅黄 */
.kb-key.is-stuck:not(.is-pressed) {
  background: #fef9c3 !important;
  border-color: #fde047 !important;
  box-shadow: 0 2px 0 #facc15, 0 3px 6px rgba(234, 179, 8, 0.15) !important;
  color: #854d0e;
}

/* Didn't register：浅红 */
.kb-key.is-missed:not(.is-pressed):not(.is-tested):not(.is-stuck) {
  background: #fee2e2 !important;
  border-color: #fca5a5 !important;
  box-shadow: 0 2px 0 #f87171, 0 3px 6px rgba(239, 68, 68, 0.12) !important;
  color: #991b1b;
}

.kb-media {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed #e5e7eb;
}
.media-title {
  display: block;
  margin-bottom: 8px;
  font-size: 12px;
  font-weight: 600;
  color: #6b7280;
}
.media-row .kb-key {
  height: 32px;
  font-size: 10px;
}

.log-card h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.log-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.log-count {
  font-size: 12px;
  color: var(--text-secondary);
}
.log-box {
  max-height: 220px;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #0f172a;
  color: #e2e8f0;
  padding: 8px 10px;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 12px;
  line-height: 1.55;
}
.log-empty {
  color: #94a3b8;
  padding: 8px 2px;
}
.log-line {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: baseline;
}
.log-time {
  color: #64748b;
  flex-shrink: 0;
}
.log-phase.down {
  color: #4ade80;
  font-weight: 700;
}
.log-phase.up {
  color: #fbbf24;
  font-weight: 700;
}
.log-phase.note {
  color: #94a3b8;
  font-weight: 600;
}
.log-src {
  font-weight: 700;
  min-width: 3.5em;
}
.log-src.src-kb {
  color: #94a3b8;
}
.log-src.src-xm {
  color: #fb923c;
}
.log-src.src-t1-usb {
  color: #38bdf8;
}
.log-src.src-t1-ble {
  color: #a78bfa;
}
.log-detail {
  color: #94a3b8;
  font-size: 11px;
}
.log-key {
  color: #93c5fd;
  font-weight: 600;
  min-width: 4em;
}
.log-code {
  color: #e2e8f0;
}
.log-vk {
  color: #a5b4fc;
}
.log-mods {
  color: #f9a8d4;
}
.log-rep {
  color: #94a3b8;
  font-size: 11px;
}
.log-raw {
  margin-top: 10px;
  width: 100%;
  min-height: 72px;
  max-height: 140px;
  resize: vertical;
  box-sizing: border-box;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: #f8fafc;
  color: var(--text);
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 11px;
  line-height: 1.45;
}

.btn {
  height: 32px;
  padding: 0 14px;
  border-radius: 6px;
  border: 1px solid transparent;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  white-space: nowrap;
}
.btn-primary {
  background: var(--primary, #3b82f6);
  color: #fff;
}
.btn-secondary {
  background: #fff;
  border-color: var(--border, #e2e8f0);
  color: var(--text, #1e293b);
}
.btn-secondary:hover {
  background: #f8fafc;
}
</style>
