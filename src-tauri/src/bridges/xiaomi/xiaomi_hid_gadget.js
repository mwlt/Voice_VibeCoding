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
