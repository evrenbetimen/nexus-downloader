// Messages for the native messaging host (crates/nexus-bridge/src/protocol.rs).

export const HOST_NAME = "dev.nexusdl.host";

export interface CapturedDownload {
  url: string;
  fileName: string | null;
  fileSize: number | null;
  referrer: string | null;
  cookies: string | null;
  userAgent: string | null;
}

export type HostMessage = { type: "ping" } | ({ type: "download" } & CapturedDownload);

export interface HostReply {
  ok: boolean;
  version?: string;
  appRunning?: boolean;
  error?: string;
}
