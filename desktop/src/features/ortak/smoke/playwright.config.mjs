import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: ["ortak.spec.mjs", "work.spec.mjs", "employee-work.spec.mjs"],
  timeout: 30_000,
  workers: 1,
  reporter: "list",
  outputDir: "../../../../test-results/ortak-smoke",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:4177",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "python3 src/features/ortak/smoke/server.py",
    cwd: "../../../..",
    url: "http://127.0.0.1:4177",
    reuseExistingServer: false,
  },
});
