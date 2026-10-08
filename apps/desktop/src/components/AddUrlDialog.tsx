import { Link, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { engine } from "../lib/bridge";

interface AddUrlDialogProps {
  initialUrl: string;
  onClose(): void;
}

export function AddUrlDialog({ initialUrl, onClose }: AddUrlDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [url, setUrl] = useState(initialUrl);
  const [fileName, setFileName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    dialogRef.current?.showModal();
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await engine.add({ url: url.trim(), fileName: fileName.trim() || null });
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
          <h2 className="flex-1 text-sm font-semibold">Add download</h2>
          <button type="button" onClick={onClose} className="rounded p-1 text-muted hover:text-ink" aria-label="Close">
            <X className="size-4" />
          </button>
        </header>
        <label className="flex flex-col gap-1.5 text-xs text-muted">
          Address
          <input
            autoFocus
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
