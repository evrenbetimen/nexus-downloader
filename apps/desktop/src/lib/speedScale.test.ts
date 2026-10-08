import { describe, expect, it } from "vitest";

import { MAX_LIMIT, MIN_LIMIT, STEPS, limitToStep, stepToLimit } from "./speedScale";

describe("speed limiter scale", () => {
  it("maps the ends of the slider", () => {
    expect(stepToLimit(0)).toBe(MIN_LIMIT);
    expect(stepToLimit(STEPS - 1)).toBe(MAX_LIMIT);
    expect(stepToLimit(STEPS)).toBeNull();
    expect(limitToStep(null)).toBe(STEPS);
  });

  it("is monotonic", () => {
    let prev = 0;
    for (let step = 0; step < STEPS; step += 1) {
      const limit = stepToLimit(step)!;
      expect(limit).toBeGreaterThan(prev);
      prev = limit;
    }
  });

  it("round-trips every step", () => {
    for (let step = 0; step <= STEPS; step += 1) {
      expect(limitToStep(stepToLimit(step))).toBe(step);
    }
  });

  it("clamps limits outside the slider range", () => {
    expect(limitToStep(1)).toBe(0);
    expect(limitToStep(10 * MAX_LIMIT)).toBe(STEPS - 1);
  });
});
