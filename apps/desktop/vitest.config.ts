import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    coverage: {
      provider: "v8",
      include: ["src/lib/format.ts", "src/lib/speedScale.ts", "src/store/**"],
      reporter: ["text-summary"],
    },
  },
});
