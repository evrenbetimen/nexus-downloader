import { Activity, Gauge, Search } from "lucide-react";

import { engine } from "../lib/bridge";
import { formatSpeed } from "../lib/format";
import { useDownloads } from "../store/downloads";

export function StatusBar({ className = "" }: { className?: string }) {
  const totalSpeed = useDownloads((s) => s.totalSpeedBps);
  const active = useDownloads((s) => Object.keys(s.live).length);
  const count = useDownloads((s) => s.byId.size);
  const limit = useDownloads((s) => s.speedLimit);
  const search = useDownloads((s) => s.search);
  const setSearch = useDownloads((s) => s.setSearch);

  return (
    <footer
      className={`flex h-9 items-center gap-5 border-t border-line bg-panel px-4 text-xs text-muted ${className}`}
    >
      <span className="flex items-center gap-1.5">
        <Activity className="size-3.5 text-accent" />
        <span className="font-mono tabular-nums text-ink">{formatSpeed(totalSpeed)}</span>
      </span>
      <span>
        {active} active · {count.toLocaleString()} total
      </span>
      {limit != null && (
        <span className="flex items-center gap-1 text-warn">
          <Gauge className="size-3.5" /> capped at {formatSpeed(limit)}
        </span>
      )}
      {!engine.native && <span className="text-warn/80">Simulated engine (browser preview)</span>}
      <div className="flex-1" />
      <label className="flex items-center gap-1.5 rounded-md bg-white/4 px-2 py-1">
        <Search className="size-3.5" />
        <input
          value={search}
          onChange={(e) => setSearch(e.currentTarget.value)}
          placeholder="Filter by name"
          className="w-44 bg-transparent text-ink outline-none placeholder:text-muted select-text"
        />
      </label>
    </footer>
  );
}
