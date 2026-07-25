<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { RouterView, useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import SideNav from "./components/SideNav.vue";

const router = useRouter();
let unlistenNav: UnlistenFn | null = null;

onMounted(async () => {
  unlistenNav = await listen<string>("navigate", (ev) => {
    if (ev.payload) router.push(ev.payload);
  });
});

onUnmounted(() => {
  unlistenNav?.();
});
</script>

<template>
  <div class="app-container">
    <SideNav />
    <main class="main-content">
      <RouterView />
    </main>
  </div>
</template>

<style>
:root {
  --primary: #1a73e8;
  --primary-dark: #1557b0;
  --bg: #f8f9fa;
  --sidebar-bg: #1e293b;
  --sidebar-text: #cbd5e1;
  --sidebar-active: #3b82f6;
  --card-bg: #ffffff;
  --border: #e2e8f0;
  --text: #1e293b;
  --text-secondary: #64748b;
  --success: #22c55e;
  --warning: #f59e0b;
  --danger: #ef4444;
  --radius: 8px;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  background: var(--bg);
  color: var(--text);
  overflow: hidden;
  height: 100vh;
}

#app {
  height: 100vh;
}

.app-container {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.main-content {
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  padding: 20px 28px;
}
</style>
