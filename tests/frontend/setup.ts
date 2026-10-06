import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.appearance;
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
