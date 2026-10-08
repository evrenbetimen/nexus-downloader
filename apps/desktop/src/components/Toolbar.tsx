import { Pause, Play, Plus, Trash } from "lucide-react";
import type { ReactNode } from "react";

import { engine } from "../lib/bridge";
import { useDownloads } from "../store/downloads";
import { ACTIVE_STATUSES } from "../types";
import { SpeedLimiter } from "./SpeedLimiter";

interface ToolbarProps {
  className?: string;
  onAddUrl(): void;
}

export function Toolbar({ className = "", onAddUrl }: ToolbarProps) {
  const selection = useDownloads((s) => s.selection);
  const byId = useDownloads((s) => s.byId);
  const selected = [...selection].flatMap((id) => byId.get(id) ?? []);
  const canPause = selected.some((d) => ACTIVE_STATUSES.has(d.status));
  const canResume = selected.some((d) => d.status === "paused" || d.status === "failed");

  const each = (fn: (id: string) => Promise<void>) => () =>
    void Promise.all(selected.map((d) => fn(d.id))).catch(console.error);

  return (
    <header
      className={`glass flex h-14 items-center gap-1.5 border-x-0 border-t-0 px-3 ${className}`}
    >
      <div className="mr-4 flex items-center gap-2 pl-1">
        <img src="/icon.png" alt="" className="size-6" />
        <span className="text-sm font-semibold tracking-wide">
          Nexus<span className="text-accent">DL</span>
        </span>
      </div>
      <ToolButton primary onClick={onAddUrl} icon={<Plus className="size-4" />}>
        Add URL
      </ToolButton>
      <ToolButton disabled={!canResume} onClick={each(engine.resume)} icon={<Play className="size-4" />}>
        Resume
      </ToolButton>
      <ToolButton disabled={!canPause} onClick={each(engine.pause)} icon={<Pause className="size-4" />}>
        Pause
      </ToolButton>
      <ToolButton
        disabled={selected.length === 0}
        onClick={each((id) => engine.remove(id, false))}
        icon={<Trash className="size-4" />}
      >
        Delete
      </ToolButton>
      <div className="flex-1" />
      <SpeedLimiter />
    </header>
  );
}

interface ToolButtonProps {
  icon: ReactNode;
  children: ReactNode;
  onClick(): void;
  disabled?: boolean;
  primary?: boolean;
}

function ToolButton({ icon, children, onClick, disabled = false, primary = false }: ToolButtonProps) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`flex h-9 items-center gap-2 rounded-lg px-3 text-[13px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-35 ${
        primary
          ? "bg-gradient-to-r from-accent to-accent-strong text-canvas hover:brightness-110"
          : "text-ink/85 hover:bg-white/6 hover:text-ink"
      }`}
    >
      {icon}
      {children}
    </button>
  );
}
