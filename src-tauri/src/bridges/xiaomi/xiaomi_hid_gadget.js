const READ_CHARACTERISTIC_IOCTL = 0x80018483;
const EXPECTED_OUTPUT_LENGTH = 9;
const HEARTBEAT_INTERVAL_MS = 5000;
const RECONNECT_DELAY_MS = 1000;

let host = "127.0.0.1";
let port = 30684;
let connection = null;
let output = null;
let writeChain = Promise.resolve();
let reconnectTimer = null;
let hookInstalled = false;

function asciiBytes(text) {
  const result = [];
  for (let index = 0; index < text.length; index++) {
    result.push(text.charCodeAt(index) & 0xff);
  }
  return result;
}

function hex(pointer, length) {
  if (pointer.isNull() || length <= 0) return "";
  const bytes = new Uint8Array(pointer.readByteArray(length));
  let result = "";
  for (let index = 0; index < bytes.length; index++) {
    result += bytes[index].toString(16).padStart(2, "0");
  }
  return result;
}

function scheduleReconnect() {
  if (reconnectTimer !== null) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectToHub();
  }, RECONNECT_DELAY_MS);
}

function markDisconnected(currentOutput) {
  if (output !== currentOutput) return;
  output = null;
  connection = null;
  scheduleReconnect();
}

function emit(payload) {
  const currentOutput = output;
  if (currentOutput === null) {
    scheduleReconnect();
    return;
  }
  const line = JSON.stringify(payload) + "\n";
  writeChain = writeChain
    .then(() => currentOutput.writeAll(asciiBytes(line)))
    .catch(() => markDisconnected(currentOutput));
}

async function connectToHub() {
  if (output !== null) return;
  try {
    const currentConnection = await Socket.connect({
      family: "ipv4",
      host: host,
      port: port
    });
    connection = currentConnection;
    output = currentConnection.output;
    emit({ kind: "ready", pid: Process.id, hook_installed: hookInstalled });
  } catch (_error) {
    connection = null;
    output = null;
    scheduleReconnect();
  }
}

function installHook() {
  if (hookInstalled) return;
  const ntdll = Process.findModuleByName("ntdll.dll");
  const target = ntdll ? ntdll.findExportByName("NtDeviceIoControlFile") : null;
  if (target === null) {
    emit({ kind: "error", message: "NtDeviceIoControlFile export not found" });
    return;
  }
  Interceptor.attach(target, {
    onEnter(args) {
      this.capture = args[5].toUInt32() === READ_CHARACTERISTIC_IOCTL;
      if (this.capture) {
        this.output = args[8];
        this.outputLength = args[9].toUInt32();
      }
    },
    onLeave(retval) {
      if (!this.capture || retval.toUInt32() !== 0 || this.output.isNull()) return;
      try {
        if (this.outputLength === EXPECTED_OUTPUT_LENGTH) {
          emit({
            kind: "gatt_read",
            raw: hex(this.output, this.outputLength)
          });
          // HID Tap 旁路抄送：清掉应用负责注入的 usage，避免原生+注入双发。
          // 方向 0x4F-52 / OK 0x28 必须清：应用侧一律 SendInput 注入（含身份映射）。
          // 始终清（不依赖 hub socket）。v1.5.9-f5-zero：脚本变更→WUDFHost 重启。
          // USB HID：F1=0x003A，F5=0x003E。重叠 bump + 源头清 = 零空窗交付。
          let rawBefore = hex(this.output, this.outputLength);
          let clearedF5 = false;
          for (let offset = 3; offset + 1 < EXPECTED_OUTPUT_LENGTH; offset += 2) {
            const usage = this.output.add(offset).readU16();
            const mapped =
              usage === 0x00f1 || // Back
              usage === 0x0028 || // OK
              usage === 0x0035 || // TV
              usage === 0x003e || // F5（语音固件泄漏；曾误写成 0x003A=F1）
              usage === 0x004a || // Home
              usage === 0x004f || usage === 0x0050 || usage === 0x0051 || usage === 0x0052 || // D-pad
              usage === 0x0065 || // Menu
              usage === 0x0066 || // Power
              usage === 0x007f || usage === 0x0080 || usage === 0x0081 ||
              usage === 0x00e2 || usage === 0x00e9 || usage === 0x00ea;
            if (mapped) {
              if (usage === 0x003e) {
                clearedF5 = true;
              }
              this.output.add(offset).writeU16(0);
            }
          }
          if (clearedF5) {
            emit({
              kind: "cleared_f5",
              raw: rawBefore,
              after: hex(this.output, this.outputLength),
              stamp: "v1.5.9-f5-zero"
            });
          }
        }
      } catch (error) {
        emit({ kind: "error", message: String(error) });
      }
    }
  });
  hookInstalled = true;
}

setInterval(() => {
  if (output === null) {
    scheduleReconnect();
  } else {
    emit({ kind: "heartbeat", pid: Process.id });
  }
}, HEARTBEAT_INTERVAL_MS);

rpc.exports = {
  async init(_stage, parameters) {
    host = (parameters && parameters.host) || host;
    port = (parameters && parameters.port) || port;
    installHook();
    await connectToHub();
  }
};

// LoadLibrary 注入后部分 Gadget 版本不会立刻调 init：脚本加载时主动挂钩并连 hub
try {
  installHook();
  connectToHub();
} catch (_error) {
  // init 路径仍会重试
}
