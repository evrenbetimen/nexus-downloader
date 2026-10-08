import { Gauge } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { formatSpeed } from "../lib/format";
import { useDownloads } from "../store/downloads";

const STEPS = 100; // the last step means "unlimited"
const MIN = 64 * 1024;
const MAX = 100 * 1024 * 1024;

/** Log scale so both 100 KB/s and 50 MB/s are easy to hit. */
function toLimit(step: number): number | null {
  if (step >= STEPS) return null;
  return Math.round(MIN * (MAX / MIN) ** (step / (STEPS - 1)));
}

function toStep(limit: number | null): number {
  if (limit == null) return STEPS;
  const ratio = Math.log(Math.min(Math.max(limit, MIN), MAX) / MIN) / Math.log(MAX / MIN);
  return Math.round(ratio * (STEPS - 1));
}

export function SpeedLimiter() {
  const speedLimit = useDownloads((s) => s.speedLimit);
  const setSpeedLimit = useDownloads((s) => s.setSpeedLimit);
  const [step, setStep] = useState(() => toStep(speedLimit));
  const pending = useRef<number | undefined>(undefined);

  useEffect(() => setStep(toStep(speedLimit)), [speedLimit]);

  // Dispatch to the token bucket while dragging, at most ~8 times a second.
  const onChange = (next: number) => {
    setStep(next);
    window.clearTimeout(pending.current);
    pending.current = window.setTimeout(() => setSpeedLimit(toLimit(next)), 120);
  };

  const limit = toLimit(step);
  return (
    <label className="flex items-center gap-3 rounded-lg bg-white/4 px-3 py-1.5" title="Global speed limit">
      <Gauge className={`size-4 ${limit == null ? "text-muted" : "text-warn"}`} />
      <input
        type="range"
        min={0}
        max={STEPS}
        value={step}
        onChange={(e) => onChange(Number(e.currentTarget.value))}
        className="w-40 accent-accent"
        aria-label="Speed limit"
      />
      <span className="w-20 text-right font-mono text-xs tabular-nums text-ink/80">
        {limit == null ? "Unlimited" : formatSpeed(limit)}
      </span>
    </label>
  );
}
