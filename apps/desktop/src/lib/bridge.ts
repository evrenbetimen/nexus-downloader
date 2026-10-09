// Typed bridge to the Rust DownloadManager (src-tauri/src/commands.rs and events.rs).
// Outside Tauri (plain `vite` in a browser) a simulated engine stands in so the
// UI can be developed and profiled without the native shell.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  CapturedDownload,
  DownloadId,
  DownloadRequest,
  DownloadSnapshot,
  ManagerEvent,
  ProgressFrame,
} from "../types";
import { createMockEngine } from "./mock";

export const EVENTS = {
  progress: "download://progress",
  lifecycle: "download://lifecycle",
  resync: "download://resync",
  captured: "download://captured",
} as const;

export interface Engine {
  readonly native: boolean;
  list(): Promise<DownloadSnapshot[]>;
  add(request: DownloadRequest): Promise<DownloadSnapshot>;
  pause(id: DownloadId): Promise<void>;
  resume(id: DownloadId): Promise<void>;
  remove(id: DownloadId, deleteFiles: boolean): Promise<void>;
  /** Bytes per second; `null` removes the cap. Applies mid-download. */
  setSpeedLimit(bytesPerSec: number | null): Promise<void>;
  getSpeedLimit(): Promise<number | null>;
  onProgress(handler: (frame: ProgressFrame) => void): Promise<UnlistenFn>;
  onLifecycle(handler: (event: ManagerEvent) => void): Promise<UnlistenFn>;
  onResync(handler: () => void): Promise<UnlistenFn>;
  /** Downloads captured by the browser extension and not yet confirmed. */
  takeCaptured(): Promise<CapturedDownload[]>;
  /** Fires when new captures are waiting; call `takeCaptured` to get them. */
  onCaptured(handler: () => void): Promise<UnlistenFn>;
}

const tauriEngine: Engine = {
  native: true,
  list: () => invoke("list_downloads"),
  add: (request) => invoke("add_download", { request }),
  pause: (id) => invoke("pause_download", { id }),
  resume: (id) => invoke("resume_download", { id }),
  remove: (id, deleteFiles) => invoke("remove_download", { id, deleteFiles }),
  setSpeedLimit: (bytesPerSec) => invoke("set_speed_limit", { bytesPerSec }),
  getSpeedLimit: () => invoke("get_speed_limit"),
  onProgress: (handler) => listen<ProgressFrame>(EVENTS.progress, (e) => handler(e.payload)),
  onLifecycle: (handler) => listen<ManagerEvent>(EVENTS.lifecycle, (e) => handler(e.payload)),
  onResync: (handler) => listen(EVENTS.resync, () => handler()),
  takeCaptured: () => invoke("take_captured_downloads"),
  onCaptured: (handler) => listen(EVENTS.captured, () => handler()),
};

export const engine: Engine = isTauri() ? tauriEngine : createMockEngine();
