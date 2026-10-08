import {
  AppWindow,
  Archive,
  CircleAlert,
  CircleCheck,
  File,
  FileText,
  Film,
  Loader,
  Music,
  Pause,
  type LucideIcon,
} from "lucide-react";

import type { DownloadStatus, FileCategory } from "../types";

const CATEGORY_ICONS: Record<FileCategory, [LucideIcon, string]> = {
  compressed: [Archive, "text-amber-300"],
  documents: [FileText, "text-sky-300"],
  music: [Music, "text-pink-300"],
  programs: [AppWindow, "text-violet-300"],
  videos: [Film, "text-emerald-300"],
  other: [File, "text-muted"],
};

export function CategoryIcon({ category }: { category: FileCategory }) {
  const [Icon, color] = CATEGORY_ICONS[category];
  return <Icon className={`size-4 shrink-0 ${color}`} />;
}

const STATUS: Record<DownloadStatus, [string, string, LucideIcon]> = {
  queued: ["Queued", "text-muted", Loader],
  connecting: ["Connecting", "text-accent", Loader],
  downloading: ["Downloading", "text-accent", Loader],
  paused: ["Paused", "text-warn", Pause],
  completed: ["Complete", "text-ok", CircleCheck],
  failed: ["Failed", "text-bad", CircleAlert],
};

export function StatusBadge({ status, error }: { status: DownloadStatus; error: string | null }) {
  const [label, color, Icon] = STATUS[status];
  const spinning = status === "connecting" || status === "downloading";
  return (
    <span className={`flex items-center gap-1.5 text-xs ${color}`} title={error ?? undefined}>
      <Icon className={`size-3.5 ${spinning ? "animate-spin [animation-duration:2.5s]" : ""}`} />
      {label}
    </span>
  );
}
