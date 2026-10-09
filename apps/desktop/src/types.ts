// Mirrors the serde types in crates/nexus-core/src/types.rs.

export type DownloadId = string;

export type DownloadStatus =
  | "queued"
  | "connecting"
  | "downloading"
  | "paused"
  | "completed"
  | "failed";

export type FileCategory =
  | "compressed"
  | "documents"
  | "music"
  | "programs"
  | "videos"
  | "other";

export interface SegmentState {
  start: number;
  end: number;
  downloaded: number;
}

export interface RequestContext {
  cookies?: string | null;
  userAgent?: string | null;
  referrer?: string | null;
  headers?: [string, string][];
}

export interface DownloadRequest {
  url: string;
  directory?: string | null;
  fileName?: string | null;
  context?: RequestContext;
}

/** A download the browser extension handed over (crates/nexus-bridge/src/protocol.rs). */
export interface CapturedDownload {
  url: string;
  fileName?: string | null;
  fileSize?: number | null;
  referrer?: string | null;
  cookies?: string | null;
  userAgent?: string | null;
}

export interface DownloadSnapshot {
  id: DownloadId;
  url: string;
  fileName: string;
  path: string;
  category: FileCategory;
  status: DownloadStatus;
  totalBytes: number | null;
  downloadedBytes: number;
  resumable: boolean;
  error: string | null;
  /** Unix epoch milliseconds. */
  createdAt: number;
}

export interface ProgressSample {
  id: DownloadId;
  status: DownloadStatus;
  totalBytes: number | null;
  downloadedBytes: number;
  speedBps: number;
  etaSecs: number | null;
  connections: number;
  segments: SegmentState[];
}

export interface ProgressFrame {
  downloads: ProgressSample[];
  totalSpeedBps: number;
}

export type ManagerEvent =
  | { kind: "added"; download: DownloadSnapshot }
  | { kind: "updated"; download: DownloadSnapshot }
  | { kind: "removed"; id: DownloadId };

export const ACTIVE_STATUSES: ReadonlySet<DownloadStatus> = new Set([
  "queued",
  "connecting",
  "downloading",
]);
