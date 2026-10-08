import {
  AppWindow,
  Archive,
  CircleCheck,
  FileText,
  Film,
  Hourglass,
  Layers,
  Music,
  type LucideIcon,
} from "lucide-react";
import { useMemo } from "react";

import { matchesFilter, useDownloads, type SidebarFilter } from "../store/downloads";

const CATEGORIES: [SidebarFilter, string, LucideIcon][] = [
  ["all", "All Downloads", Layers],
  ["compressed", "Compressed", Archive],
  ["documents", "Documents", FileText],
  ["music", "Music", Music],
  ["programs", "Programs", AppWindow],
  ["videos", "Videos", Film],
];

const QUEUES: [SidebarFilter, string, LucideIcon][] = [
  ["unfinished", "Unfinished", Hourglass],
  ["finished", "Finished", CircleCheck],
];

export function Sidebar() {
  const byId = useDownloads((s) => s.byId);
  const counts = useMemo(() => {
    const all = [...byId.values()];
    return Object.fromEntries(
      [...CATEGORIES, ...QUEUES].map(([key]) => [key, all.filter((d) => matchesFilter(d, key)).length]),
    ) as Record<SidebarFilter, number>;
  }, [byId]);

  return (
    <nav className="flex flex-col gap-6 overflow-y-auto border-r border-line bg-panel/60 px-3 py-4">
      <Group title="Categories" items={CATEGORIES} counts={counts} />
      <Group title="Queues" items={QUEUES} counts={counts} />
    </nav>
  );
}

interface GroupProps {
  title: string;
  items: [SidebarFilter, string, LucideIcon][];
  counts: Record<SidebarFilter, number>;
}

function Group({ title, items, counts }: GroupProps) {
  const filter = useDownloads((s) => s.filter);
  const setFilter = useDownloads((s) => s.setFilter);
  return (
    <section>
      <h2 className="mb-1.5 px-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-muted">{title}</h2>
      <ul className="flex flex-col gap-0.5">
        {items.map(([key, label, Icon]) => {
          const active = key === filter;
          return (
            <li key={key}>
              <button
                type="button"
                onClick={() => setFilter(key)}
                className={`flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-[13px] transition-colors ${
                  active ? "bg-accent/12 text-accent" : "text-ink/75 hover:bg-white/5 hover:text-ink"
                }`}
              >
                <Icon className="size-4 shrink-0" />
                <span className="flex-1 text-left">{label}</span>
                <span className="font-mono text-[11px] tabular-nums text-muted">
                  {counts[key].toLocaleString()}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
