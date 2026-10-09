import { Globe, Link, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { engine } from "../lib/bridge";
import { formatBytes } from "../lib/format";
import type { CapturedDownload } from "../types";

interface AddUrlDialogProps {
  /** A typed or dropped address (manual add) or a browser capture. */
  initial: string | CapturedDownload;
  onClose(): void;
}

function hostOf(url: string | null | undefined): string | null {
  try {
    return url ? new URL(url).host : null;
  } catch {
    return null;
  }
}

export function AddUrlDialog({ initial, onClose }: AddUrlDialogProps) {
  const captured = typeof initial === "string" ? null : initial;
  const dialogRef = useRef<HTMLDialogElement>(null);
  const addressRef = useRef<HTMLInputElement>(null);
  const [url, setUrl] = useState(captured ? captured.url : (initial as string));
  const [fileName, setFileName] = useState(captured?.fileName ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    dialogRef.current?.showModal();
    // showModal() focuses the first focusable element (the close button), so
    // move focus to the address field explicitly.
    addressRef.current?.focus();
    addressRef.current?.select();
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await engine.add({
        url: url.trim(),
        fileName: fileName.trim() || null,
        // Reuse the browser session so signed-in and hotlink-protected links work.
        ...(captured && {
          context: {
            cookies: captured.cookies ?? null,
            referrer: captured.referrer ?? null,
            userAgent: captured.userAgent ?? null,
          },
        }),
      });
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog
      ref={dialogRef}
      onClose={onClose}
      className="m-auto w-[520px] rounded-2xl border border-line bg-raised p-0 text-ink shadow-2xl backdrop:bg-black/60 backdrop:backdrop-blur-sm"
    >
      <form onSubmit={submit} className="flex flex-col gap-4 p-5">
        <header className="flex items-center gap-2">
          <Link className="size-4 text-accent" />
          <h2 className="flex-1 text-sm font-semibold">{captured ? "Download from browser" : "Add download"}</h2>
          <button type="button" onClick={onClose} className="rounded p-1 text-muted hover:text-ink" aria-label="Close">
            <X className="size-4" />
          </button>
        </header>
        {captured && (
          <p className="flex items-center gap-2 rounded-lg bg-accent/10 px-3 py-2 text-xs text-ink/80">
            <Globe className="size-3.5 shrink-0 text-accent" />
            <span className="truncate">
              Captured from {hostOf(captured.referrer) ?? "your browser"}
              {captured.fileSize ? ` · ${formatBytes(captured.fileSize)}` : ""}
              {captured.cookies ? " · signed-in session" : ""}
            </span>
          </p>
        )}
        <label className="flex flex-col gap-1.5 text-xs text-muted">
          Address
          <input
            ref={addressRef}
            required
            type="url"
            value={url}
            onChange={(e) => setUrl(e.currentTarget.value)}
            placeholder="https://"
            className="select-text rounded-lg border border-line bg-canvas px-3 py-2 font-mono text-[13px] text-ink outline-none focus:border-accent"
          />
        </label>
        <label className="flex flex-col gap-1.5 text-xs text-muted">
          Save as (optional)
          <input
            value={fileName}
            onChange={(e) => setFileName(e.currentTarget.value)}
            placeholder="Detected from the server"
            className="select-text rounded-lg border border-line bg-canvas px-3 py-2 text-[13px] text-ink outline-none focus:border-accent"
          />
        </label>
        {error && <p className="text-xs text-bad">{error}</p>}
        <footer className="flex justify-end gap-2">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-[13px] text-ink/80 hover:bg-white/6">
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy || !url.trim()}
            className="rounded-lg bg-gradient-to-r from-accent to-accent-strong px-4 py-2 text-[13px] font-medium text-canvas disabled:opacity-40"
          >
            Start download
          </button>
        </footer>
      </form>
    </dialog>
  );
}
