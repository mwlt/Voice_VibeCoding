import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [vue()],
  // Relative base so production asset/chunk paths work under Tauri custom protocol
  base: "./",
  clearScreen: false,
  // 2430: 1420/1430 sit in Windows excluded ranges (Hyper-V/WinNAT often reserves 1382–1481)
  server: {
    port: 2430,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 2431 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
