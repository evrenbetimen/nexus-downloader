import { useEffect, useState } from "react";

import { AddUrlDialog } from "./components/AddUrlDialog";
import { DetailsPanel } from "./components/DetailsPanel";
import { DownloadTable } from "./components/DownloadTable";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";
import { connectEngine } from "./store/downloads";

export function App() {
  const [addUrl, setAddUrl] = useState<string | null>(null);

  useEffect(() => connectEngine(), []);

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
      {addUrl !== null && <AddUrlDialog initialUrl={addUrl} onClose={() => setAddUrl(null)} />}
    </div>
  );
}
