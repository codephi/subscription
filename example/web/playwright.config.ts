import { defineConfig } from "@playwright/test"

export default defineConfig({
  testDir: "./e2e",
  use: { baseURL: "http://127.0.0.1:5174", headless: true },
  webServer: { command: "cd .. && npx vite --config web/vite.config.ts", url: "http://127.0.0.1:5174", reuseExistingServer: !process.env.CI, timeout: 30_000 },
})
