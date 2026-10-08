import { beforeEach, describe, expect, it, vi } from "vitest";

import type { DownloadSnapshot, ProgressSample } from "../types";

const engine = vi.hoisted(() => ({
  native: false,
  setSpeedLimit: vi.fn(() => Promise.resolve()),
}));
vi.mock("../lib/bridge", () => ({ engine }));

const { liveSegments, matchesFilter, useDownloads } = await import("./downloads");

function snapshot(overrides: Partial<DownloadSnapshot> = {}): DownloadSnapshot {
  return {
    id: "a",
    url: "https://example.com/a.zip",
    fileName: "a.zip",
    path: "/tmp/a.zip",
    category: "compressed",
    status: "downloading",
    totalBytes: 1000,
    downloadedBytes: 0,
    resumable: true,
    error: null,
    createdAt: 1,
    ...overrides,
  };
}

function sample(overrides: Partial<ProgressSample> = {}): ProgressSample {
  return {
    id: "a",
    status: "downloading",
    totalBytes: 1000,
    downloadedBytes: 400,
    speedBps: 100,
    etaSecs: 6,
    connections: 4,
    segments: [{ start: 0, end: 1000, downloaded: 400 }],
    ...overrides,
  };
}

const initial = useDownloads.getState();

beforeEach(() => {
  useDownloads.setState(initial, true);
  liveSegments.clear();
  engine.setSpeedLimit.mockClear();
});

describe("downloads store", () => {
  it("hydrates from the engine list", () => {
    useDownloads.getState().hydrate([snapshot(), snapshot({ id: "b" })]);
    expect(useDownloads.getState().byId.size).toBe(2);
  });

  it("applies lifecycle events", () => {
    const s = useDownloads.getState();
    s.applyEvent({ kind: "added", download: snapshot() });
    s.applyEvent({ kind: "updated", download: snapshot({ status: "paused" }) });
    expect(useDownloads.getState().byId.get("a")?.status).toBe("paused");
    s.select("a", "replace");
    s.applyEvent({ kind: "removed", id: "a" });
    const after = useDownloads.getState();
    expect(after.byId.size).toBe(0);
    expect(after.selection.size).toBe(0);
    expect(after.focusedId).toBeNull();
  });

  it("keeps live stats only for running downloads", () => {
    const s = useDownloads.getState();
    s.applyEvent({ kind: "added", download: snapshot() });
    s.applyFrame({ downloads: [sample()], totalSpeedBps: 100 });
    expect(useDownloads.getState().live.a?.connections).toBe(4);
    expect(useDownloads.getState().totalSpeedBps).toBe(100);
    expect(liveSegments.get("a")).toHaveLength(1);

    s.applyEvent({ kind: "updated", download: snapshot({ status: "completed" }) });
    expect(useDownloads.getState().live.a).toBeUndefined();

    s.applyFrame({ downloads: [], totalSpeedBps: 0 });
    expect(useDownloads.getState().live).toEqual({});
  });

  it("does not re-create unrelated records on progress frames", () => {
    const s = useDownloads.getState();
    s.hydrate([snapshot(), snapshot({ id: "b", status: "completed" })]);
    const byId = useDownloads.getState().byId;
    s.applyFrame({ downloads: [sample()], totalSpeedBps: 100 });
    expect(useDownloads.getState().byId).toBe(byId);
  });

  it("selects with replace and toggle", () => {
    const s = useDownloads.getState();
    s.select("a", "replace");
    s.select("b", "toggle");
    expect([...useDownloads.getState().selection]).toEqual(["a", "b"]);
    s.select("a", "toggle");
    expect([...useDownloads.getState().selection]).toEqual(["b"]);
    s.select("c", "replace");
    expect([...useDownloads.getState().selection]).toEqual(["c"]);
    expect(useDownloads.getState().focusedId).toBe("c");
  });

  it("dispatches speed limits to the engine", () => {
    useDownloads.getState().setSpeedLimit(1024);
    expect(useDownloads.getState().speedLimit).toBe(1024);
    expect(engine.setSpeedLimit).toHaveBeenCalledWith(1024);
  });
});

describe("matchesFilter", () => {
  it("filters by category and queue", () => {
    const done = snapshot({ status: "completed", category: "videos" });
    expect(matchesFilter(done, "all")).toBe(true);
    expect(matchesFilter(done, "videos")).toBe(true);
    expect(matchesFilter(done, "music")).toBe(false);
    expect(matchesFilter(done, "finished")).toBe(true);
    expect(matchesFilter(done, "unfinished")).toBe(false);
    expect(matchesFilter(snapshot({ status: "failed" }), "unfinished")).toBe(true);
  });
});
