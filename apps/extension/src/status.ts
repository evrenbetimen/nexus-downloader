// What the popup says about the connection to the app.

import type { HostReply } from "./protocol";

export function describe(reply: HostReply | Error): { state: string; text: string } {
  if (reply instanceof Error || !reply.ok) {
    return {
      state: "missing",
      text: "NexusDL app not found. Install it and open it once so the browser can reach it.",
    };
  }
  const version = reply.version ? ` ${reply.version}` : "";
  return reply.appRunning
    ? { state: "connected", text: `Connected to NexusDL${version}.` }
    : { state: "installed", text: `NexusDL${version} is installed. Captured downloads will open it.` };
}
