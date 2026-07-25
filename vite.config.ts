import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [vue()],
  // Relative base so production asset/chunk paths work under Tauri custom protocol
  base: "./",
  clearScreen: false,
  // Use 1430 — sibling project xiaomi_remote_2_pro_rust already binds 1420
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1431 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
