<script setup lang="ts">
import { onMounted, computed } from "vue";
import { useBridgeStore } from "../stores/bridge";
import { useConfigStore } from "../stores/config";
import DeviceStatus from "../components/DeviceStatus.vue";
import KeyBindingEditor from "../components/KeyBindingEditor.vue";

const bridge = useBridgeStore();
const configStore = useConfigStore();
const type = "hanvon" as const;

const device = computed(() => bridge.devices[type]);
const config = computed(() => configStore.configs[type]);

onMounted(async () => {
  await Promise.all([bridge.refreshStatus(type), configStore.loadConfig(type)]);
});

function toggleConnection() {
  if (device.value.status === "Connected") bridge.stopBridge(type);
  else bridge.startBridge(type);
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h2>✏️ 汉王 V60 语音笔 <span class="wip-tag">开发中</span></h2>
      <DeviceStatus
        :status="device.status"
        :loading="bridge.loading[type]"
        @toggle="toggleConnection"
      />
    </header>

    <div class="page-body">
      <section class="card">
        <h3>设备信息</h3>
        <div class="info-grid">
          <div class="info-item">
            <span class="info-label">设备名称</span>
            <span class="info-value">{{ device.device_name || "Hanvon V60" }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">连接方式</span>
            <span class="info-value">USB HID</span>
          </div>
          <div class="info-item">
            <span class="info-label">VID/PID</span>
            <span class="info-value">27B9 / 02A2</span>
          </div>
          <div class="info-item">
            <span class="info-label">HID 驱动</span>
            <span class="info-value">纯 Rust 实现（无需厂商 DLL）</span>
          </div>
        </div>
      </section>

      <section class="card" v-if="config">
        <h3>笔键映射 (3 键)</h3>
        <p class="help-text">
          V60 语音笔配有 3 个物理按键：麦克风、上翻页、下翻页。
          默认映射：麦克风→右Alt、上翻页→End+退格、下翻页→回车。
        </p>
        <KeyBindingEditor
          :bridge-type="type"
          :config="config"
          @save="(cfg) => configStore.saveConfig(type, cfg)"
        />
      </section>

      <section class="card">
        <h3>语音设置</h3>
        <p class="help-text">
          V60 笔通过原生 USB 麦克风进行语音输入。麦克风键用于切换语音会话。
        </p>
        <div class="form-row">
          <label>语音快捷键</label>
          <span class="voice-hotkey">{{ config?.voice_hotkey?.join(" + ") || "右Alt" }}</span>
        </div>
      </section>

      <section class="card">
        <h3>状态日志</h3>
        <div class="log-area">
          <p class="log-entry">等待设备连接...</p>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.page { max-width: 800px; }
.page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px; }
.page-header h2 { font-size: 20px; font-weight: 600; }
.wip-tag {
  display: inline-block;
  margin-left: 8px;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
  vertical-align: middle;
  color: #92400e;
  background: #fef3c7;
  border: 1px solid #fde68a;
}
.page-body { display: flex; flex-direction: column; gap: 16px; }

.card {
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px;
}
.card h3 { font-size: 15px; font-weight: 600; margin-bottom: 14px; }

.info-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.info-item { display: flex; flex-direction: column; gap: 4px; }
.info-label { font-size: 12px; color: var(--text-secondary); }
.info-value { font-size: 14px; font-weight: 500; }

.help-text { font-size: 13px; color: var(--text-secondary); margin-bottom: 12px; line-height: 1.5; }

.form-row { display: flex; align-items: center; gap: 12px; padding: 8px 0; }
.form-row label { font-size: 13px; font-weight: 500; min-width: 100px; }
.voice-hotkey {
  font-family: monospace;
  background: #f1f5f9;
  padding: 4px 10px;
  border-radius: 4px;
  font-size: 13px;
}

.log-area {
  background: #f1f5f9;
  border-radius: 4px;
  padding: 12px;
  max-height: 150px;
  overflow-y: auto;
  font-family: monospace;
  font-size: 12px;
}
.log-entry { color: var(--text-secondary); }
</style>
