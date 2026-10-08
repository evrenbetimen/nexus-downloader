import { create } from "zustand";

import { engine } from "../lib/bridge";
import {
  ACTIVE_STATUSES,
  type DownloadId,
  type DownloadSnapshot,
  type FileCategory,
  type ManagerEvent,
  type ProgressFrame,
  type SegmentState,
} from "../types";

export type SidebarFilter = "all" | FileCategory | "unfinished" | "finished";

/** Fast-changing numbers for one running download. */
export interface LiveStats {
  downloadedBytes: number;
  totalBytes: number | null;
  speedBps: number;
  etaSecs: number | null;
  connections: number;
}

interface DownloadsState {
  /** Replaced only on lifecycle events (add / status change / remove). */
  byId: ReadonlyMap<DownloadId, DownloadSnapshot>;
  /** Only running downloads; replaced every progress frame. */
  live: Readonly<Record<DownloadId, LiveStats>>;
  totalSpeedBps: number;
  speedLimit: number | null;
  filter: SidebarFilter;
  search: string;
  selection: ReadonlySet<DownloadId>;
  focusedId: DownloadId | null;

  hydrate(list: DownloadSnapshot[]): void;
  applyEvent(event: ManagerEvent): void;
  applyFrame(frame: ProgressFrame): void;
  setFilter(filter: SidebarFilter): void;
  setSearch(search: string): void;
  select(id: DownloadId, mode: "replace" | "toggle"): void;
  setSpeedLimit(bytesPerSec: number | null): void;
}

/**
 * Segment byte maps live outside React state: the canvas visualiser reads
 * them every animation frame, so they never trigger renders.
 */
export const liveSegments = new Map<DownloadId, SegmentState[]>();

export const useDownloads = create<DownloadsState>()((set, get) => ({
  byId: new Map(),
  live: {},
  totalSpeedBps: 0,
  speedLimit: null,
  filter: "all",
  search: "",
  selection: new Set(),
  focusedId: null,

  hydrate: (list) => set({ byId: new Map(list.map((d) => [d.id, d])) }),

  applyEvent: (event) => {
    const byId = new Map(get().byId);
    if (event.kind === "removed") {
      byId.delete(event.id);
      liveSegments.delete(event.id);
      const selection = new Set(get().selection);
      selection.delete(event.id);
      const focusedId = get().focusedId === event.id ? null : get().focusedId;
      set({ byId, selection, focusedId });
      return;
    }
    byId.set(event.download.id, event.download);
    const patch: Partial<DownloadsState> = { byId };
    if (!ACTIVE_STATUSES.has(event.download.status) && event.download.id in get().live) {
      const { [event.download.id]: _finished, ...live } = get().live;
      patch.live = live;
    }
    set(patch);
  },

  applyFrame: (frame) => {
    const live: Record<DownloadId, LiveStats> = {};
    for (const d of frame.downloads) {
      live[d.id] = {
        downloadedBytes: d.downloadedBytes,
        totalBytes: d.totalBytes,
        speedBps: d.speedBps,
        etaSecs: d.etaSecs,
        connections: d.connections,
      };
      if (d.segments.length > 0) liveSegments.set(d.id, d.segments);
    }
    set({ live, totalSpeedBps: frame.totalSpeedBps });
  },

  setFilter: (filter) => set({ filter }),
  setSearch: (search) => set({ search }),

  select: (id, mode) => {
    if (mode === "replace") {
      set({ selection: new Set([id]), focusedId: id });
      return;
    }
    const selection = new Set(get().selection);
    if (selection.has(id)) selection.delete(id);
    else selection.add(id);
    set({ selection, focusedId: id });
  },

  setSpeedLimit: (speedLimit) => {
    set({ speedLimit });
    void engine.setSpeedLimit(speedLimit);
  },
}));

export function matchesFilter(d: DownloadSnapshot, filter: SidebarFilter): boolean {
  switch (filter) {
    case "all":
      return true;
    case "finished":
      return d.status === "completed";
    case "unfinished":
      return d.status !== "completed";
    default:
      return d.category === filter;
  }
}

/** Wires the store to the engine; returns a disposer. */
export function connectEngine(): () => void {
  const store = useDownloads.getState();
  const reload = () => engine.list().then(store.hydrate);
  const subscriptions = [
    engine.onLifecycle(store.applyEvent),
    engine.onProgress(store.applyFrame),
    engine.onResync(() => void reload()),
  ];
  void reload();
  void engine.getSpeedLimit().then((speedLimit) => useDownloads.setState({ speedLimit }));
  return () => subscriptions.forEach((p) => void p.then((unlisten) => unlisten()));
}
