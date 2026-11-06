import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

export default defineConfig({
  root: "web",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": new URL("./src", import.meta.url).pathname } },
  server: { host: "127.0.0.1", port: 5174, strictPort: true, proxy: { "/api": "http://127.0.0.1:3001" } },
  test: { environment: "jsdom", include: ["src/**/*.test.{ts,tsx}"] },
})
