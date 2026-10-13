import { useEffect, useState } from "react";

import { AddUrlDialog } from "./components/AddUrlDialog";
import { DetailsPanel } from "./components/DetailsPanel";
import { DownloadTable } from "./components/DownloadTable";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";
import { engine } from "./lib/bridge";
import { connectEngine } from "./store/downloads";
import type { CapturedDownload } from "./types";

export function App() {
  const [addUrl, setAddUrl] = useState<string | null>(null);
  // Browser captures wait their turn; each one gets its own confirmation.
  const [captured, setCaptured] = useState<{ seq: number; download: CapturedDownload }[]>([]);

  useEffect(() => connectEngine(), []);

  useEffect(() => {
    let seq = 0;
    const drain = () =>
      void engine.takeCaptured().then((items) => {
        if (items.length) setCaptured((q) => [...q, ...items.map((download) => ({ seq: ++seq, download }))]);
      });
    const unlisten = engine.onCaptured(drain);
    // Captures that started the app arrived before this listener existed.
    drain();
    return () => void unlisten.then((stop) => stop());
  }, []);

  // Dropping a link anywhere on the window opens the add dialog prefilled.
  useEffect(() => {
    const over = (e: DragEvent) => e.preventDefault();
    const drop = (e: DragEvent) => {
      const text = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain");
      const url = text?.split(/\s+/).find((t) => /^https?:\/\//i.test(t));
      if (url) {
        e.preventDefault();
        setAddUrl(url);
      }
    };
    window.addEventListener("dragover", over);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragover", over);
      window.removeEventListener("drop", drop);
    };
  }, []);

  return (
    <div className="grid h-full grid-cols-[232px_1fr] grid-rows-[auto_1fr_auto]">
      <Toolbar className="col-span-2" onAddUrl={() => setAddUrl("")} />
      <Sidebar />
      <main className="flex min-h-0 min-w-0 flex-col">
        <DownloadTable />
        <DetailsPanel />
      </main>
      <StatusBar className="col-span-2" />
      {addUrl !== null ? (
        <AddUrlDialog initial={addUrl} onClose={() => setAddUrl(null)} />
      ) : (
        captured[0] && (
          <AddUrlDialog
            key={captured[0].seq}
            initial={captured[0].download}
            onClose={() => setCaptured((q) => q.slice(1))}
          />
        )
      )}
    </div>
  );
}
