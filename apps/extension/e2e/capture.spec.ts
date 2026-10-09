// The built extension in a real Chromium, talking to a stub native host.

import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium, expect, test as base, type BrowserContext, type Worker } from "@playwright/test";

const here = dirname(fileURLToPath(import.meta.url));
const extensionDir = join(here, "../dist/chromium");
const EXTENSION_ID = "aiahehffjnedhfbjllinegdigmhpkghl";
const ISO_SIZE = 8 * 1024 * 1024;

interface HostLog {
  message: Record<string, unknown>;
  args: string[];
}

interface Fixtures {
  site: string;
  hostReply: Record<string, unknown>;
  profile: { context: BrowserContext; worker: Worker; hostLog: () => Promise<HostLog[]> };
}

/** Serves a page with a session cookie and a few download links. */
function startSite(): Promise<Server> {
  const server = createServer((req, res) => {
    if (req.url === "/") {
      res.setHeader("Set-Cookie", "sid=s3cr3t; Path=/");
      res.setHeader("Content-Type", "text/html");
      res.end(`<a id="iso" href="/files/big.iso">ISO</a> <a id="png" href="/pic.png" download>PNG</a>`);
    } else if (req.url === "/files/big.iso") {
      res.writeHead(200, {
        "Content-Type": "application/x-iso9660-image",
        "Content-Disposition": 'attachment; filename="big.iso"',
        "Content-Length": ISO_SIZE,
      });
      // Trickle so the transfer is still running when the extension cancels it.
      let sent = 0;
      const timer = setInterval(() => {
        if (res.destroyed || sent >= ISO_SIZE) return clearInterval(timer), res.end();
        res.write(Buffer.alloc(64 * 1024));
        sent += 64 * 1024;
      }, 20);
      res.on("close", () => clearInterval(timer));
    } else if (req.url === "/pic.png") {
      res.writeHead(200, { "Content-Type": "image/png" });
      res.end(Buffer.alloc(512));
    } else {
      res.writeHead(404).end();
    }
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

const test = base.extend<Fixtures>({
  site: async ({}, use) => {
    const server = await startSite();
    await use(`http://127.0.0.1:${(server.address() as AddressInfo).port}`);
    server.close();
  },
  hostReply: [{ ok: true, version: "test", appRunning: true }, { option: true }],
  profile: async ({ hostReply }, use) => {
    const root = await mkdtemp(join(tmpdir(), "nexusdl-ext-"));
    const userData = join(root, "profile");
    const hostDir = join(root, "host");
    const log = join(hostDir, "messages.jsonl");
    await mkdir(join(userData, "NativeMessagingHosts"), { recursive: true });
    await mkdir(hostDir);
    await copyFile(join(here, "stub-host.mjs"), join(hostDir, "stub-host.mjs"));
    await writeFile(join(hostDir, "stub-config.json"), JSON.stringify({ log, reply: hostReply }));
    // Chromium looks for per-user host manifests inside its profile directory.
    await writeFile(
      join(userData, "NativeMessagingHosts", "dev.nexusdl.host.json"),
      JSON.stringify({
        name: "dev.nexusdl.host",
        description: "test stub",
        path: join(hostDir, "stub-host.mjs"),
        type: "stdio",
        allowed_origins: [`chrome-extension://${EXTENSION_ID}/`],
      }),
    );

    const context = await chromium.launchPersistentContext(userData, {
      // Extensions need full Chromium; the default headless shell can't load them.
      ...(process.env.PW_CHROMIUM_PATH ? { executablePath: process.env.PW_CHROMIUM_PATH } : { channel: "chromium" }),
      args: [`--disable-extensions-except=${extensionDir}`, `--load-extension=${extensionDir}`],
      acceptDownloads: true,
    });
    const worker = context.serviceWorkers()[0] ?? (await context.waitForEvent("serviceworker"));
    const hostLog = async () =>
      (await readFile(log, "utf8").catch(() => ""))
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line) as HostLog);
    await use({ context, worker, hostLog });
    await context.close();
    await rm(root, { recursive: true, force: true });
  },
});

const downloads = (worker: Worker) =>
  worker.evaluate(() => chrome.downloads.search({}).then((items) => items.map((d) => ({ url: d.url, state: d.state }))));

test("the extension ID matches the one the app registers", async ({ profile }) => {
  expect(new URL(profile.worker.url()).host).toBe(EXTENSION_ID);
});

test("captures a download with the page's session and cancels the browser copy", async ({ profile, site }) => {
  const page = await profile.context.newPage();
  await page.goto(site);
  await page.click("#iso");

  await expect.poll(profile.hostLog).toHaveLength(1);
  const [{ message, args }] = (await profile.hostLog()) as [HostLog];
  expect(message).toEqual({
    type: "download",
    url: `${site}/files/big.iso`,
    fileName: null,
    fileSize: ISO_SIZE,
    referrer: `${site}/`,
    cookies: "sid=s3cr3t",
    userAgent: expect.stringContaining("Chrome/"),
  });
  // Chromium names the calling extension when it starts the host.
  expect(args[0]).toBe(`chrome-extension://${EXTENSION_ID}/`);
  await expect.poll(() => downloads(profile.worker)).toEqual([]);
});

test("small images stay in the browser", async ({ profile, site }) => {
  const page = await profile.context.newPage();
  await page.goto(site);
  const download = page.waitForEvent("download");
  await page.click("#png");
  expect(await (await download).failure()).toBeNull();
  expect(await profile.hostLog()).toEqual([]);
});

test.describe("when the app refuses", () => {
  test.use({ hostReply: { ok: false, error: "disk full" } });

  test("the browser downloads the file itself and flags the toolbar button", async ({ profile, site }) => {
    const page = await profile.context.newPage();
    await page.goto(site);
    await page.click("#iso");

    await expect.poll(profile.hostLog).toHaveLength(1);
    await expect
      .poll(() => downloads(profile.worker))
      .toEqual([{ url: `${site}/files/big.iso`, state: expect.stringMatching(/in_progress|complete/) }]);
    expect(await profile.worker.evaluate(() => chrome.action.getBadgeText({}))).toBe("!");
    expect(await profile.worker.evaluate(() => chrome.action.getTitle({}))).toBe("NexusDL: disk full");
    // The browser's retry was not captured a second time.
    expect(await profile.hostLog()).toHaveLength(1);
  });
});

test("the popup shows the connection and saves settings", async ({ profile, site }) => {
  const popup = await profile.context.newPage();
  await popup.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
  await expect(popup.getByRole("status")).toHaveText("Connected to NexusDL test.");
  expect((await profile.hostLog())[0]?.message).toEqual({ type: "ping" });

  // Switching capture off keeps downloads in the browser.
  await popup.getByLabel("Capture downloads").uncheck();
  const page = await profile.context.newPage();
  await page.goto(site);
  const download = page.waitForEvent("download");
  await page.click("#iso");
  await download;
  expect(await profile.hostLog()).toHaveLength(1);
});
