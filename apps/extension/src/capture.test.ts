import { describe, expect, it, vi } from "vitest";

import { cookieHeader, createCapture, type BrowserApi, type DownloadItemLike } from "./capture";
import { DEFAULT_SETTINGS } from "./policy";
import type { HostMessage, HostReply } from "./protocol";

function fakeBrowser(reply: HostReply | Error = { ok: true, appRunning: true }) {
  const api = {
    settings: vi.fn(async () => DEFAULT_SETTINGS),
    cancel: vi.fn(async (_id: number) => {}),
    cookies: vi.fn(async (_url: string): Promise<string | null> => "sid=abc; theme=dark"),
    sendToHost: vi.fn(async (_m: HostMessage) => {
      if (reply instanceof Error) throw reply;
      return reply;
    }),
    download: vi.fn(async (_url: string) => {}),
    report: vi.fn((_error: string | null) => {}),
    userAgent: "Mozilla/5.0 Test",
  } satisfies BrowserApi;
  return { api, capture: createCapture(api) };
}

const item = (overrides: Partial<DownloadItemLike> = {}): DownloadItemLike => ({
  id: 7,
  url: "https://example.com/redirect/42",
  finalUrl: "https://cdn.example.com/files/big.iso",
  referrer: "https://example.com/downloads",
  filename: "",
  mime: "application/x-iso9660-image",
  totalBytes: 4_000_000_000,
  ...overrides,
});

describe("capturing browser downloads", () => {
  it("cancels the browser download and hands it over with the session", async () => {
    const { api, capture } = fakeBrowser();
    expect(await capture.onCreated(item())).toBe(true);
    expect(api.cancel).toHaveBeenCalledWith(7);
    expect(api.cookies).toHaveBeenCalledWith("https://cdn.example.com/files/big.iso");
    expect(api.sendToHost).toHaveBeenCalledWith({
      type: "download",
      url: "https://cdn.example.com/files/big.iso",
      fileName: null,
      fileSize: 4_000_000_000,
      referrer: "https://example.com/downloads",
      cookies: "sid=abc; theme=dark",
      userAgent: "Mozilla/5.0 Test",
    });
    expect(api.download).not.toHaveBeenCalled();
    expect(api.report).toHaveBeenCalledWith(null);
  });

  it("uses Firefox's full path as the file name", async () => {
    const { api, capture } = fakeBrowser();
    await capture.onCreated(item({ filename: "/home/me/Downloads/big (1).iso", totalBytes: -1 }));
    expect(api.sendToHost.mock.calls[0]?.[0]).toMatchObject({ fileName: "big (1).iso", fileSize: null });
  });

  it("ignores downloads the policy keeps in the browser", async () => {
    const { api, capture } = fakeBrowser();
    expect(await capture.onCreated(item({ finalUrl: "https://x.example/pic.png", mime: "image/png" }))).toBe(false);
    expect(api.cancel).not.toHaveBeenCalled();
    expect(api.sendToHost).not.toHaveBeenCalled();
  });

  it("gives the download back to the browser when the app is missing", async () => {
    const { api, capture } = fakeBrowser(new Error("Specified native messaging host not found."));
    expect(await capture.onCreated(item())).toBe(false);
    expect(api.download).toHaveBeenCalledWith("https://cdn.example.com/files/big.iso");
    expect(api.report).toHaveBeenCalledWith("Specified native messaging host not found.");

    // The browser's own retry isn't captured again...
    api.sendToHost.mockClear();
    expect(await capture.onCreated(item({ id: 8, url: "https://cdn.example.com/files/big.iso", finalUrl: "" }))).toBe(false);
    expect(api.sendToHost).not.toHaveBeenCalled();
    // ...but the next real download is.
    await capture.onCreated(item({ id: 9 }));
    expect(api.sendToHost).toHaveBeenCalledTimes(1);
  });

  it("gives the download back when the app refuses it", async () => {
    const { api, capture } = fakeBrowser({ ok: false, error: "disk full" });
    await capture.onCreated(item());
    expect(api.download).toHaveBeenCalledOnce();
    expect(api.report).toHaveBeenCalledWith("disk full");
  });

  it("leaves finished downloads alone when cancelling fails", async () => {
    const { api, capture } = fakeBrowser();
    api.cancel.mockRejectedValueOnce(new Error("Download must be in progress"));
    expect(await capture.onCreated(item())).toBe(false);
    expect(api.sendToHost).not.toHaveBeenCalled();
  });

  it("still sends the download when cookies can't be read", async () => {
    const { api, capture } = fakeBrowser();
    api.cookies.mockRejectedValueOnce(new Error("firstPartyDomain required"));
    await capture.onCreated(item());
    expect(api.sendToHost.mock.calls[0]?.[0]).toMatchObject({ cookies: null });
  });
});

describe("context menu", () => {
  it("sends links with the page as referrer", async () => {
    const { api, capture } = fakeBrowser();
    expect((await capture.sendLink("https://x.example/a.pdf", "https://x.example/docs")).ok).toBe(true);
    expect(api.sendToHost.mock.calls[0]?.[0]).toMatchObject({
      type: "download",
      url: "https://x.example/a.pdf",
      referrer: "https://x.example/docs",
    });
    expect(api.cancel).not.toHaveBeenCalled();
  });

  it("refuses non-http links and reports host errors", async () => {
    const { api, capture } = fakeBrowser(new Error("host exited"));
    expect((await capture.sendLink("javascript:void(0)", undefined)).ok).toBe(false);
    expect(api.sendToHost).not.toHaveBeenCalled();
    expect((await capture.sendLink("https://x.example/a.pdf", undefined)).error).toBe("host exited");
    expect(api.report).toHaveBeenLastCalledWith("host exited");
  });
});

describe("cookieHeader", () => {
  it("joins cookies like a browser would", () => {
    expect(cookieHeader([])).toBeNull();
    expect(cookieHeader([{ name: "a", value: "1" }, { name: "", value: "bare" }])).toBe("a=1; bare");
  });
});
