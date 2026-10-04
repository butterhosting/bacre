import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "*.test.ts",
  outputDir: "./.test-results",
  timeout: 60_000,
  expect: { timeout: 20_000 },
  reporter: [["html", { outputFolder: "./.playwright-report", open: "never" }], ...(process.env.CI ? [["github"] as const] : [])],
  /* The tests share one daemon and one sandbox, so one at a time */
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  use: {
    baseURL: "http://localhost:3000",
    timezoneId: "UTC",
    trace: process.env.CI ? "retain-on-failure" : "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  /* The sandbox directory has to exist before it is mounted */
  webServer: {
    cwd: "..",
    command: [
      "mkdir -p e2e/.sandbox",
      `HOST_UID=${process.getuid!()} HOST_GID=${process.getgid!()} docker compose -f compose.e2e.yaml up --build`,
    ].join(" && "),
    stdout: "pipe",
    url: "http://localhost:3000/health",
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
    gracefulShutdown: {
      signal: "SIGTERM",
      timeout: 5000,
    },
  },
});
