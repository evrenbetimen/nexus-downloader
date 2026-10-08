// Log-scale mapping between the speed-limit slider and bytes per second, so
// both 100 KB/s and 50 MB/s are easy to hit.

export const STEPS = 100; // the last step means "unlimited"
export const MIN_LIMIT = 64 * 1024;
export const MAX_LIMIT = 100 * 1024 * 1024;

export function stepToLimit(step: number): number | null {
  if (step >= STEPS) return null;
  return Math.round(MIN_LIMIT * (MAX_LIMIT / MIN_LIMIT) ** (Math.max(0, step) / (STEPS - 1)));
}

export function limitToStep(limit: number | null): number {
  if (limit == null) return STEPS;
  const clamped = Math.min(Math.max(limit, MIN_LIMIT), MAX_LIMIT);
  return Math.round((Math.log(clamped / MIN_LIMIT) / Math.log(MAX_LIMIT / MIN_LIMIT)) * (STEPS - 1));
}
