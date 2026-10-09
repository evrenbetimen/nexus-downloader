import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  // Each test launches its own browser profile.
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  timeout: 30_000,
});
