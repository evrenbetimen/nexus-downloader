// Takes downloads over from the browser. Browser APIs are injected so the
// logic can be tested without one (see capture.test.ts).

import { baseName, isHttpUrl, shouldCapture, type Settings } from "./policy";
import type { HostMessage, HostReply } from "./protocol";

/** The parts of `downloads.DownloadItem` used here (Chrome and Firefox agree on them). */
export interface DownloadItemLike {
  id: number;
  url: string;
  finalUrl?: string;
  referrer?: string;
  filename?: string;
  mime?: string;
  totalBytes?: number;
  fileSize?: number;
  incognito?: boolean;
}

export interface BrowserApi {
  settings(): Promise<Settings>;
  /** Stops the browser's own download and removes it from its list. */
  cancel(id: number): Promise<void>;
  /** `Cookie` header for `url`, or null when there are none. */
  cookies(url: string): Promise<string | null>;
  sendToHost(message: HostMessage): Promise<HostReply>;
  /** Lets the browser download `url` itself after all. */
  download(url: string): Promise<void>;
  /** Shows the outcome on the toolbar button. */
  report(error: string | null): void;
  userAgent: string;
}

export function createCapture(api: BrowserApi) {
  // Downloads we hand back to the browser must not be captured again.
  const handedBack = new Set<string>();

  async function send(message: HostMessage): Promise<HostReply> {
    try {
      return await api.sendToHost(message);
    } catch (e) {
      // The host isn't registered (app never opened) or crashed.
      return { ok: false, error: e instanceof Error ? e.message : String(e) };
    }
  }

  async function capture(
    url: string,
    details: { fileName?: string | null | undefined; fileSize?: number | null | undefined; referrer?: string | null | undefined },
  ) {
    return send({
      type: "download",
      url,
      fileName: details.fileName ?? null,
      fileSize: details.fileSize ?? null,
      referrer: details.referrer || null,
      cookies: await api.cookies(url).catch(() => null),
      userAgent: api.userAgent,
    });
  }

  return {
    /** `downloads.onCreated` listener. Resolves to whether NexusDL took the download. */
    async onCreated(item: DownloadItemLike): Promise<boolean> {
      const url = item.finalUrl || item.url;
      if (handedBack.delete(url) || handedBack.delete(item.url)) return false;
      const size = item.totalBytes && item.totalBytes > 0 ? item.totalBytes : item.fileSize;
      const fileName = item.filename ? baseName(item.filename) : undefined;
      const decision = shouldCapture(
        { url, fileName, mime: item.mime, size, incognito: item.incognito },
        await api.settings(),
      );
      if (!decision.capture) return false;

      try {
        await api.cancel(item.id);
      } catch {
        // Already finished (tiny files) or gone: leave it to the browser.
        return false;
      }
      const reply = await capture(url, { fileName, fileSize: size && size > 0 ? size : null, referrer: item.referrer });
      if (reply.ok) {
        api.report(null);
        return true;
      }
      // Never lose a download: give it back to the browser.
      handedBack.add(url);
      await api.download(url);
      api.report(reply.error ?? "NexusDL refused the download");
      return false;
    },

    /** Context menu "Download with NexusDL". */
    async sendLink(url: string, pageUrl: string | undefined): Promise<HostReply> {
      if (!isHttpUrl(url)) {
        const reply = { ok: false, error: "Only http(s) links can be sent to NexusDL" };
        api.report(reply.error);
        return reply;
      }
      const reply = await capture(url, { referrer: pageUrl ?? null });
      api.report(reply.ok ? null : (reply.error ?? "NexusDL refused the download"));
      return reply;
    },
  };
}

/** Serializes cookies as a `Cookie` header value. */
export function cookieHeader(cookies: { name: string; value: string }[]): string | null {
  if (!cookies.length) return null;
  return cookies.map((c) => (c.name ? `${c.name}=${c.value}` : c.value)).join("; ");
}
