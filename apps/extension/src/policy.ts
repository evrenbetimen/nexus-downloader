// Which browser downloads NexusDL takes over, IDM-style: by file type, with
// an optional minimum size.

export interface Settings {
  enabled: boolean;
  /** Lower-case file extensions without the dot. */
  extensions: string[];
  /** Downloads the browser knows to be smaller than this stay in the browser. */
  minSizeBytes: number;
}

export const DEFAULT_EXTENSIONS = [
  "3gp", "7z", "aac", "apk", "appimage", "avi", "bin", "bz2", "deb", "dmg", "exe", "flac",
  "flv", "gz", "img", "iso", "m4a", "m4v", "mkv", "mov", "mp3", "mp4", "mpeg", "mpg", "msi",
  "msix", "ogg", "opus", "pdf", "pkg", "rar", "rpm", "tar", "tgz", "wav", "webm", "wma", "wmv",
  "xz", "zip", "zst",
];

export const DEFAULT_SETTINGS: Settings = {
  enabled: true,
  extensions: DEFAULT_EXTENSIONS,
  minSizeBytes: 0,
};

/** Binary types worth taking over even when the URL has no telling extension. */
const BINARY_MIME = [
  /^video\//,
  /^audio\//,
  /^application\/(octet-stream|zip|gzip|pdf|x-7z-compressed|vnd\.rar|x-rar(-compressed)?|x-tar|x-xz|zstd)$/,
  /^application\/(x-msdownload|x-msi|x-apple-diskimage|x-iso9660-image|vnd\.debian\.binary-package|x-rpm|vnd\.android\.package-archive)$/,
];

/** What the browser tells us about a download. */
export interface Candidate {
  url: string;
  /** Name or full path the browser picked, if any. */
  fileName?: string | undefined;
  mime?: string | undefined;
  /** Bytes; unknown when absent or not positive. */
  size?: number | undefined;
  incognito?: boolean | undefined;
}

/** Lower-case extension of a file name, path or URL path, or "". */
export function fileExtension(nameOrUrl: string): string {
  let path = nameOrUrl;
  try {
    path = new URL(nameOrUrl).pathname;
  } catch {
    // A plain name or a filesystem path.
  }
  const base = path.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

/** Last path segment of a browser-reported file path. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function isHttpUrl(url: string): boolean {
  return /^https?:\/\//i.test(url);
}

export type Decision = { capture: true } | { capture: false; reason: string };

export function shouldCapture(candidate: Candidate, settings: Settings): Decision {
  if (!settings.enabled) return { capture: false, reason: "capture is off" };
  // Private windows stay private: their cookies never leave the browser.
  if (candidate.incognito) return { capture: false, reason: "private window" };
  if (!isHttpUrl(candidate.url)) return { capture: false, reason: "not an http(s) download" };
  const size = candidate.size ?? 0;
  if (size > 0 && size < settings.minSizeBytes) return { capture: false, reason: "below the minimum size" };

  const extensions = new Set(settings.extensions);
  const named = candidate.fileName ? fileExtension(candidate.fileName) : "";
  if (named) {
    return extensions.has(named) ? { capture: true } : { capture: false, reason: `.${named} is not captured` };
  }
  const fromUrl = fileExtension(candidate.url);
  if (fromUrl && extensions.has(fromUrl)) return { capture: true };
  const mime = candidate.mime?.split(";")[0]?.trim().toLowerCase() ?? "";
  if (mime && BINARY_MIME.some((re) => re.test(mime))) return { capture: true };
  return { capture: false, reason: "not a captured file type" };
}

/** Parses the popup's free-form list ("zip, .ISO  mkv") into extensions. */
export function parseExtensions(text: string): string[] {
  const list = text
    .split(/[\s,;]+/)
    .map((e) => e.replace(/^\./, "").toLowerCase())
    .filter((e) => /^[a-z0-9]+$/.test(e));
  return [...new Set(list)].sort();
}

/** Fills in defaults for settings saved by older versions (or none at all). */
export function normalizeSettings(stored: Partial<Settings> | undefined): Settings {
  return {
    enabled: typeof stored?.enabled === "boolean" ? stored.enabled : DEFAULT_SETTINGS.enabled,
    extensions: Array.isArray(stored?.extensions) ? stored.extensions : DEFAULT_SETTINGS.extensions,
    minSizeBytes:
      typeof stored?.minSizeBytes === "number" && stored.minSizeBytes >= 0
        ? stored.minSizeBytes
        : DEFAULT_SETTINGS.minSizeBytes,
  };
}
