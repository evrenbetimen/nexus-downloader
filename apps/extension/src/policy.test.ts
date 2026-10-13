import { describe, expect, it } from "vitest";

import {
  DEFAULT_SETTINGS,
  fileExtension,
  normalizeSettings,
  parseExtensions,
  shouldCapture,
  type Candidate,
} from "./policy";

const decide = (c: Candidate, settings = DEFAULT_SETTINGS) => shouldCapture(c, settings).capture;

describe("fileExtension", () => {
  it("reads names, paths and URLs", () => {
    expect(fileExtension("Movie.Final.MKV")).toBe("mkv");
    expect(fileExtension("C:\\Users\\me\\Downloads\\setup.exe")).toBe("exe");
    expect(fileExtension("/home/me/Downloads/archive.tar.gz")).toBe("gz");
    expect(fileExtension("https://cdn.example.com/a/b/file.zip?token=1.2#x.y")).toBe("zip");
    expect(fileExtension("https://example.com/download?id=7")).toBe("");
    expect(fileExtension(".bashrc")).toBe("");
  });
});

describe("shouldCapture", () => {
  it("takes listed file types, by name first, then URL", () => {
    expect(decide({ url: "https://x.example/f/ubuntu.iso" })).toBe(true);
    expect(decide({ url: "https://x.example/get?id=1", fileName: "/tmp/Album.FLAC" })).toBe(true);
    // The name the browser chose beats a misleading URL.
    expect(decide({ url: "https://x.example/file.zip", fileName: "notes.txt" })).toBe(false);
  });

  it("falls back to binary MIME types for opaque URLs", () => {
    expect(decide({ url: "https://x.example/dl?id=42", mime: "application/octet-stream" })).toBe(true);
    expect(decide({ url: "https://x.example/dl?id=42", mime: "video/mp4; codecs=avc1" })).toBe(true);
    expect(decide({ url: "https://x.example/dl?id=42", mime: "image/png" })).toBe(false);
    expect(decide({ url: "https://x.example/page", mime: "text/html" })).toBe(false);
  });

  it("leaves browser-only, private and small downloads alone", () => {
    expect(shouldCapture({ url: "blob:https://x.example/1", fileName: "a.zip" }, DEFAULT_SETTINGS)).toEqual({
      capture: false,
      reason: "not an http(s) download",
    });
    expect(decide({ url: "data:application/zip;base64,AA==" })).toBe(false);
    expect(decide({ url: "https://x.example/a.zip", incognito: true })).toBe(false);
    const min = { ...DEFAULT_SETTINGS, minSizeBytes: 1024 * 1024 };
    expect(decide({ url: "https://x.example/a.zip", size: 1000 }, min)).toBe(false);
    expect(decide({ url: "https://x.example/a.zip", size: 5_000_000 }, min)).toBe(true);
    // Unknown sizes (-1 / 0) are not "small".
    expect(decide({ url: "https://x.example/a.zip", size: -1 }, min)).toBe(true);
  });

  it("does nothing when switched off", () => {
    expect(decide({ url: "https://x.example/a.zip" }, { ...DEFAULT_SETTINGS, enabled: false })).toBe(false);
  });
});

describe("settings", () => {
  it("parses the popup's extension list", () => {
    expect(parseExtensions(" zip, .ISO;mkv\n\nmkv  bad-ext *")).toEqual(["iso", "mkv", "zip"]);
  });

  it("fills defaults for missing or corrupt settings", () => {
    expect(normalizeSettings(undefined)).toEqual(DEFAULT_SETTINGS);
    expect(normalizeSettings({ enabled: false })).toEqual({ ...DEFAULT_SETTINGS, enabled: false });
    expect(normalizeSettings({ minSizeBytes: -5, extensions: "zip" as unknown as string[] })).toEqual(DEFAULT_SETTINGS);
  });
});
