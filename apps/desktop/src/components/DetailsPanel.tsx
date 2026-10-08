import { useDownloads } from "../store/downloads";
import { formatBytes, formatEta, formatSpeed } from "../lib/format";
import { SegmentMap } from "./SegmentMap";

export function DetailsPanel() {
  const download = useDownloads((s) => (s.focusedId ? s.byId.get(s.focusedId) : undefined));
  const live = useDownloads((s) => (s.focusedId ? s.live[s.focusedId] : undefined));

  if (!download) {
    return (
      <section className="flex h-52 shrink-0 items-center justify-center border-t border-line bg-panel/40 text-sm text-muted">
        Select a download to see its segment map.
      </section>
    );
  }

  const done = live?.downloadedBytes ?? download.downloadedBytes;
  const total = live?.totalBytes ?? download.totalBytes;
  return (
    <section className="grid h-52 shrink-0 grid-cols-[260px_1fr] gap-5 border-t border-line bg-panel/40 p-4">
      <dl className="grid grid-cols-[auto_1fr] content-start gap-x-4 gap-y-1.5 text-xs">
        <dt className="col-span-2 mb-1 truncate text-sm font-medium text-ink" title={download.path}>
          {download.fileName}
        </dt>
        <Stat label="Downloaded" value={`${formatBytes(done)} / ${formatBytes(total)}`} />
        <Stat label="Speed" value={formatSpeed(live?.speedBps)} />
        <Stat label="Time left" value={formatEta(live?.etaSecs)} />
        <Stat label="Connections" value={live ? String(live.connections) : "—"} />
        <Stat label="Resumable" value={download.resumable ? "Yes" : "No"} />
        {download.error && <Stat label="Error" value={download.error} tone="text-bad" />}
      </dl>
      <div className="min-w-0 overflow-hidden rounded-lg border border-line bg-canvas/60 p-2.5">
        <SegmentMap
          downloadId={download.id}
          totalBytes={total}
          complete={download.status === "completed"}
        />
      </div>
    </section>
  );
}

function Stat({ label, value, tone = "text-ink/90" }: { label: string; value: string; tone?: string }) {
  return (
    <>
      <dt className="text-muted">{label}</dt>
      <dd className={`truncate font-mono tabular-nums ${tone}`} title={value}>
        {value}
      </dd>
    </>
  );
}
