import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "path";

// Tauri 期望前端固定端口
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // Tauri 使用固定端口，防止热重载时重新连接
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 不监听 Rust 源码
      ignored: ["**/src-tauri/**"],
    },
  },
}));
