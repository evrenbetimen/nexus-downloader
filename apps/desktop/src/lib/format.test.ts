import { describe, expect, it } from "vitest";

import { formatBytes, formatEta, formatSpeed, percent } from "./format";

describe("formatBytes", () => {
  it.each([
    [0, "0 B"],
    [1023, "1023 B"],
    [1024, "1.00 KB"],
    [1536, "1.50 KB"],
    [10 * 1024 * 1024, "10.0 MB"],
    [250 * 1024 * 1024, "250 MB"],
    [3.5 * 1024 ** 3, "3.50 GB"],
    [2 * 1024 ** 5, "2048 TB"],
  ])("%d bytes → %s", (bytes, text) => {
    expect(formatBytes(bytes)).toBe(text);
  });

  it("shows a dash for unknown sizes", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });
});

describe("formatSpeed", () => {
  it("adds per-second units and hides zero", () => {
    expect(formatSpeed(5 * 1024 * 1024)).toBe("5.00 MB/s");
    expect(formatSpeed(0)).toBe("—");
    expect(formatSpeed(null)).toBe("—");
  });
});

describe("formatEta", () => {
  it.each([
    [0, "0:00"],
    [59.4, "0:59"],
    [61, "1:01"],
    [3600, "1:00:00"],
    [3 * 3600 + 7 * 60 + 5, "3:07:05"],
  ])("%d s → %s", (secs, text) => {
    expect(formatEta(secs)).toBe(text);
  });

  it("handles unknown and nonsense values", () => {
    expect(formatEta(null)).toBe("—");
    expect(formatEta(Number.POSITIVE_INFINITY)).toBe("—");
    expect(formatEta(-5)).toBe("0:00");
  });
});

describe("percent", () => {
  it("clamps and needs a known total", () => {
    expect(percent(50, 200)).toBe(25);
    expect(percent(300, 200)).toBe(100);
    expect(percent(5, null)).toBeNull();
    expect(percent(5, 0)).toBeNull();
  });
});
