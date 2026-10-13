// Browser-only simulated engine: a 10,000-row history plus a few live
// transfers whose segments split and fill like the real engine's.

import type { UnlistenFn } from "@tauri-apps/api/event";

import type {
  CapturedDownload,
  DownloadRequest,
  DownloadSnapshot,
  FileCategory,
  ManagerEvent,
  ProgressFrame,
  ProgressSample,
  SegmentState,
} from "../types";
import type { Engine } from "./bridge";

const SAMPLE_FILES: [string, FileCategory][] = [
  ["ubuntu-26.04-desktop-amd64.iso", "compressed"],
  ["Blender-5.2-linux-x64.tar.xz", "compressed"],
  ["Annual-Report-2026.pdf", "documents"],
  ["lofi-set-vol3.flac", "music"],
  ["NexusSetup-x64.msi", "programs"],
  ["keynote-4k-hdr.mkv", "videos"],
  ["dataset-shard-0042.parquet", "other"],
];

interface Sim {
  snapshot: DownloadSnapshot;
  segments: SegmentState[];
  bandwidth: number;
}

function category(name: string): FileCategory {
  const ext = (n: string) => n.slice(n.lastIndexOf("."));
  return SAMPLE_FILES.find(([n]) => ext(n) === ext(name))?.[1] ?? "other";
}

export function createMockEngine(): Engine {
  const sims = new Map<string, Sim>();
  const progressHandlers = new Set<(f: ProgressFrame) => void>();
  const lifecycleHandlers = new Set<(e: ManagerEvent) => void>();
  const capturedHandlers = new Set<() => void>();
  let captured: CapturedDownload[] = [];
  let limit: number | null = null;
  let seq = 0;

  // Lets end-to-end tests play the browser extension.
  (window as { __nexusdlCapture?: (d: CapturedDownload) => void }).__nexusdlCapture = (d) => {
    captured.push(d);
    capturedHandlers.forEach((h) => h());
  };

  const make = (name: string, total: number, status: DownloadSnapshot["status"], ago: number) => {
    seq += 1;
    const snapshot: DownloadSnapshot = {
      id: `mock-${seq}`,
      url: `https://cdn.example.com/files/${encodeURIComponent(name)}`,
      fileName: name,
      path: `~/Downloads/${name}`,
      category: category(name),
      status,
      totalBytes: total,
      downloadedBytes: status === "completed" ? total : 0,
      resumable: true,
      error: null,
      createdAt: Date.now() - ago,
    };
    sims.set(snapshot.id, {
      snapshot,
      segments: [{ start: 0, end: total, downloaded: snapshot.downloadedBytes }],
      bandwidth: 2e6 + Math.random() * 18e6,
    });
    return snapshot;
  };

  for (let i = 0; i < 10_000; i += 1) {
    const [name] = SAMPLE_FILES[i % SAMPLE_FILES.length]!;
    make(name.replace(/(\.[^.]+)$/, `-${i}$1`), 5e6 + ((i * 7919) % 4000) * 1e6, "completed", (i + 10) * 60_000);
  }
  for (const [name] of SAMPLE_FILES.slice(0, 4)) {
    make(name, 400e6 + Math.random() * 3e9, "downloading", 0);
  }

  const emit = (event: ManagerEvent) => lifecycleHandlers.forEach((h) => h(event));

  const split = (sim: Sim) => {
    if (sim.segments.length >= 32) return;
    let best = sim.segments[0]!;
    for (const s of sim.segments) {
      if (s.end - (s.start + s.downloaded) > best.end - (best.start + best.downloaded)) best = s;
    }
    const remaining = best.end - (best.start + best.downloaded);
    if (remaining < 4e6) return;
    const mid = best.start + best.downloaded + Math.floor(remaining / 2);
    sim.segments.push({ start: mid, end: best.end, downloaded: 0 });
    best.end = mid;
  };

  let last = performance.now();
  const tick = (now: number) => {
    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;
    const active = [...sims.values()].filter((s) => s.snapshot.status === "downloading");
    const share = limit ? limit / Math.max(1, active.length) : Infinity;
    const samples: ProgressSample[] = [];
    for (const sim of active) {
      if (Math.random() < 0.05) split(sim);
      const open = sim.segments.filter((s) => s.downloaded < s.end - s.start);
      const speed = Math.min(share, sim.bandwidth * (0.85 + Math.random() * 0.3));
      const perSegment = (speed * dt) / Math.max(1, open.length);
      for (const s of open) s.downloaded = Math.min(s.end - s.start, s.downloaded + perSegment);
      const done = sim.segments.reduce((n, s) => n + s.downloaded, 0);
      const total = sim.snapshot.totalBytes ?? 0;
      sim.snapshot.downloadedBytes = done;
      if (done >= total) {
        sim.snapshot.status = "completed";
        emit({ kind: "updated", download: { ...sim.snapshot } });
        continue;
      }
      samples.push({
        id: sim.snapshot.id,
        status: "downloading",
        totalBytes: total,
        downloadedBytes: done,
        speedBps: speed,
        etaSecs: (total - done) / speed,
        connections: open.length,
        segments: sim.segments.map((s) => ({ ...s })),
      });
    }
    const frame = { downloads: samples, totalSpeedBps: samples.reduce((n, s) => n + s.speedBps, 0) };
    progressHandlers.forEach((h) => h(frame));
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  const sub = <T>(set: Set<T>, handler: T): Promise<UnlistenFn> => {
    set.add(handler);
    return Promise.resolve(() => void set.delete(handler));
  };
  const setStatus = (id: string, status: DownloadSnapshot["status"]) => {
    const sim = sims.get(id);
    if (!sim || sim.snapshot.status === "completed") return Promise.resolve();
    sim.snapshot.status = status;
    emit({ kind: "updated", download: { ...sim.snapshot } });
    return Promise.resolve();
  };

  return {
    native: false,
    list: () => Promise.resolve([...sims.values()].map((s) => ({ ...s.snapshot }))),
    add: (request) => {
      (window as { __nexusdlLastRequest?: DownloadRequest }).__nexusdlLastRequest = request;
      const name = request.fileName || decodeURIComponent(request.url.split("/").pop() || "download");
      const snapshot = make(name, 50e6 + Math.random() * 900e6, "downloading", 0);
      emit({ kind: "added", download: { ...snapshot } });
      return Promise.resolve({ ...snapshot });
    },
    pause: (id) => setStatus(id, "paused"),
    resume: (id) => setStatus(id, "downloading"),
    remove: (id) => {
      sims.delete(id);
      emit({ kind: "removed", id });
      return Promise.resolve();
    },
    setSpeedLimit: (bytesPerSec) => {
      limit = bytesPerSec;
      return Promise.resolve();
    },
    getSpeedLimit: () => Promise.resolve(limit),
    onProgress: (h) => sub(progressHandlers, h),
    onLifecycle: (h) => sub(lifecycleHandlers, h),
    onResync: () => Promise.resolve(() => {}),
    takeCaptured: () => {
      const taken = captured;
      captured = [];
      return Promise.resolve(taken);
    },
    onCaptured: (h) => sub(capturedHandlers, h),
  };
}
