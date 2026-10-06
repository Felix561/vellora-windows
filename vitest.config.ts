import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["tests/frontend/**/*.test.{ts,tsx}"],
    setupFiles: ["./tests/frontend/setup.ts"],
    clearMocks: true,
    restoreMocks: true,
    // Keep regression checks light on developer PCs and Windows CI runners.
    maxWorkers: 2,
  },
});
