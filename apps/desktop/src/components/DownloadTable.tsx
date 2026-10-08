import {
  columnSizingFeature,
  createColumnHelper,
  createSortedRowModel,
  rowSortingFeature,
  sortFn_alphanumeric,
  sortFn_basic,
  tableFeatures,
  useTable,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { useMemo, useRef } from "react";

import { formatBytes, formatEta, formatSpeed, percent } from "../lib/format";
import { matchesFilter, useDownloads } from "../store/downloads";
import type { DownloadSnapshot } from "../types";
import { CategoryIcon, StatusBadge } from "./badges";

const ROW_HEIGHT = 38;

const features = tableFeatures({
  columnSizingFeature,
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
  sortFns: { alphanumeric: sortFn_alphanumeric, basic: sortFn_basic },
});

const helper = createColumnHelper<typeof features, DownloadSnapshot>();

const columns = helper.columns([
  helper.accessor("fileName", {
    header: "Name",
    size: 340,
    sortFn: "alphanumeric",
    cell: (ctx) => (
      <span className="flex min-w-0 items-center gap-2.5">
        <CategoryIcon category={ctx.row.original.category} />
        <span className="truncate" title={ctx.row.original.url}>
          {ctx.getValue()}
        </span>
      </span>
    ),
  }),
  helper.accessor("totalBytes", {
    header: "Size",
    size: 96,
    sortFn: "basic",
    cell: (ctx) => <span className="font-mono text-xs">{formatBytes(ctx.getValue())}</span>,
  }),
  helper.display({
    id: "progress",
    header: "Progress",
    size: 200,
    cell: (ctx) => <ProgressCell download={ctx.row.original} />,
  }),
  helper.display({
    id: "speed",
    header: "Speed",
    size: 104,
    cell: (ctx) => <LiveCell id={ctx.row.original.id} field="speed" />,
  }),
  helper.display({
    id: "eta",
    header: "Time left",
    size: 88,
    cell: (ctx) => <LiveCell id={ctx.row.original.id} field="eta" />,
  }),
  helper.display({
    id: "connections",
    header: "Conn.",
    size: 64,
    cell: (ctx) => <LiveCell id={ctx.row.original.id} field="connections" />,
  }),
  helper.accessor("status", {
    header: "Status",
    size: 118,
    sortFn: "alphanumeric",
    cell: (ctx) => <StatusBadge status={ctx.getValue()} error={ctx.row.original.error} />,
  }),
  helper.accessor("createdAt", {
    header: "Added",
    size: 132,
    sortFn: "basic",
    cell: (ctx) => (
      <span className="text-xs text-muted">
        {new Date(ctx.getValue()).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" })}
      </span>
    ),
  }),
]);

export function DownloadTable() {
  const byId = useDownloads((s) => s.byId);
  const filter = useDownloads((s) => s.filter);
  const search = useDownloads((s) => s.search);

  const data = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return [...byId.values()].filter(
      (d) => matchesFilter(d, filter) && (!needle || d.fileName.toLowerCase().includes(needle)),
    );
  }, [byId, filter, search]);

  const table = useTable({
    features,
    columns,
    data,
    getRowId: (row) => row.id,
    initialState: { sorting: [{ id: "createdAt", desc: true }] },
  });

  const scrollRef = useRef<HTMLDivElement>(null);
  const rows = table.getRowModel().rows;
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: (index) => rows[index]?.id ?? index,
    overscan: 12,
  });
  const totalWidth = table.getTotalSize();

  return (
    <div ref={scrollRef} role="grid" aria-rowcount={rows.length} className="min-h-0 flex-1 overflow-auto">
      <div style={{ width: totalWidth, minWidth: "100%" }}>
        {table.getHeaderGroups().map((group) => (
          <div
            key={group.id}
            role="row"
            className="sticky top-0 z-10 flex border-b border-line bg-panel/95 backdrop-blur"
          >
            {group.headers.map((header) => {
              const sorted = header.column.getIsSorted();
              return (
                <div
                  key={header.id}
                  role="columnheader"
                  style={{ width: header.getSize() }}
                  onClick={header.column.getToggleSortingHandler()}
                  className={`flex h-9 shrink-0 items-center gap-1 whitespace-nowrap px-3 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted ${
                    header.column.getCanSort() ? "cursor-pointer hover:text-ink" : ""
                  }`}
                >
                  <table.FlexRender header={header} />
                  {sorted === "asc" && <ArrowUp className="size-3" />}
                  {sorted === "desc" && <ArrowDown className="size-3" />}
                </div>
              );
            })}
          </div>
        ))}

        <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
          {virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index];
            if (!row) return null;
            return (
              <SelectableRow key={row.id} id={row.id} top={item.start}>
                {row.getAllCells().map((cell) => (
                  <div
                    key={cell.id}
                    role="gridcell"
                    style={{ width: cell.column.getSize() }}
                    className="flex shrink-0 items-center overflow-hidden whitespace-nowrap px-3"
                  >
                    <table.FlexRender cell={cell} />
                  </div>
                ))}
              </SelectableRow>
            );
          })}
        </div>
      </div>
      {rows.length === 0 && (
        <p className="py-24 text-center text-sm text-muted">
          No downloads here yet. Add a URL or drop a link onto the window.
        </p>
      )}
    </div>
  );
}

function SelectableRow({ id, top, children }: { id: string; top: number; children: React.ReactNode }) {
  const selected = useDownloads((s) => s.selection.has(id));
  const select = useDownloads((s) => s.select);
  return (
    <div
      role="row"
      aria-selected={selected}
      onClick={(e) => select(id, e.ctrlKey || e.metaKey ? "toggle" : "replace")}
      style={{ height: ROW_HEIGHT, transform: `translateY(${top}px)` }}
      className={`absolute left-0 flex w-full border-b border-line/50 text-[13px] ${
        selected ? "bg-accent/10" : "hover:bg-white/3"
      }`}
    >
      {children}
    </div>
  );
}

function ProgressCell({ download }: { download: DownloadSnapshot }) {
  const live = useDownloads((s) => s.live[download.id]);
  const done = live?.downloadedBytes ?? download.downloadedBytes;
  const total = live?.totalBytes ?? download.totalBytes;
  const pct = download.status === "completed" ? 100 : percent(done, total);
  const color =
    download.status === "failed" ? "bg-bad" : download.status === "paused" ? "bg-warn" : "bg-accent";
  return (
    <span className="flex w-full items-center gap-2">
      <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-white/6">
        <span
          className={`block h-full rounded-full ${color} ${pct == null && live ? "animate-pulse" : ""}`}
          style={{ width: `${pct ?? (live ? 100 : 0)}%` }}
        />
      </span>
      <span className="w-11 text-right font-mono text-[11px] tabular-nums text-ink/70">
        {pct == null ? formatBytes(done) : `${pct.toFixed(1)}%`}
      </span>
    </span>
  );
}

function LiveCell({ id, field }: { id: string; field: "speed" | "eta" | "connections" }) {
  const live = useDownloads((s) => s.live[id]);
  if (!live) return <span className="text-muted">—</span>;
  const text =
    field === "speed"
      ? formatSpeed(live.speedBps)
      : field === "eta"
        ? formatEta(live.etaSecs)
        : String(live.connections);
  return <span className="font-mono text-xs tabular-nums">{text}</span>;
}
