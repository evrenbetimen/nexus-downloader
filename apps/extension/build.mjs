// Builds the extension for Chromium browsers and Firefox into dist/<browser>.

import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const root = dirname(fileURLToPath(import.meta.url));
const desktop = join(root, "../desktop/src-tauri");
// One version for the app and the extension.
const { version } = JSON.parse(await readFile(join(desktop, "tauri.conf.json"), "utf8"));

// Public half of the signing key: pins the Chromium extension ID to
// aiahehffjnedhfbjllinegdigmhpkghl, which the app's host manifest allows.
const CHROMIUM_KEY =
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAscIcp9KhQZYgWZzTNDmu9UH7MoVDZ1p65CaapRFIKUTkYfBbkhPwf0tCJQEXaKzKfj4aYbh0NvC+UFTd0L3NTgE2enoAoEq09cIQ0GvlR87tNeiAyauzrNUVSDwL6CCv8sFN1xXBNNzVzeCHgzD1bGarRHOeINpyIp4pzUMPEjJ52uJKp/zugUt0lK+Wnjmm9VNQ6HGxXM8noIqSXNlIcciCtxqdIkIxLbXO//8Gu8k7Uw826GO8cWPUsaKkcx2uiq7m/iFdQwp0W7PFG75fqEnxUWgK8VDiZhvpv7TSUZScOfdN095gnsw5DlA7VsAjkaZRP4ScScQLoDlkkLQqGwIDAQAB";

const common = {
  manifest_version: 3,
  name: "NexusDL",
  version,
  description: "Send browser downloads to the NexusDL download manager.",
  icons: { 32: "icons/32.png", 128: "icons/128.png" },
  action: { default_popup: "popup.html", default_title: "NexusDL", default_icon: { 32: "icons/32.png" } },
  permissions: ["downloads", "cookies", "nativeMessaging", "storage", "contextMenus"],
  host_permissions: ["<all_urls>"],
};

const targets = {
  chromium: {
    ...common,
    key: CHROMIUM_KEY,
    minimum_chrome_version: "116",
    background: { service_worker: "background.js" },
  },
  firefox: {
    ...common,
    background: { scripts: ["background.js"] },
    browser_specific_settings: { gecko: { id: "nexusdl@nexusdl.dev", strict_min_version: "128.0" } },
  },
};

for (const [name, manifest] of Object.entries(targets)) {
  const out = join(root, "dist", name);
  await rm(out, { recursive: true, force: true });
  await mkdir(join(out, "icons"), { recursive: true });
  await build({
    entryPoints: { background: join(root, "src/background.ts"), popup: join(root, "src/popup.ts") },
    outdir: out,
    bundle: true,
    format: "iife",
    target: name === "firefox" ? "firefox128" : "chrome116",
    minify: true,
    logLevel: "warning",
  });
  await cp(join(root, "static"), out, { recursive: true });
  await cp(join(desktop, "icons/32x32.png"), join(out, "icons/32.png"));
  await cp(join(desktop, "icons/128x128.png"), join(out, "icons/128.png"));
  await writeFile(join(out, "manifest.json"), JSON.stringify(manifest, null, 2));
  console.log(`built dist/${name} (v${version})`);
}
