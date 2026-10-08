import { useEffect, useRef } from "react";

import { liveSegments } from "../store/downloads";
import type { SegmentState } from "../types";

const CELL = 9;
const GAP = 2;

interface SegmentMapProps {
  downloadId: string;
  totalBytes: number | null;
  /** Draw every cell as finished (completed downloads have no live map). */
  complete: boolean;
}

/**
 * IDM-style block map: the file is a grid of equal byte ranges, each cell
 * filling as its bytes are flushed to disk, with every connection's write
 * head highlighted. Drawn on a canvas from a requestAnimationFrame loop that
 * reads the segment map outside React, so it never causes re-renders.
 */
export function SegmentMap({ downloadId, totalBytes, complete }: SegmentMapProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    let width = 0;
    let height = 0;
    let lastDrawn: SegmentState[] | undefined;
    let dirty = true;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const dpr = window.devicePixelRatio || 1;
      width = entry.contentRect.width;
      height = entry.contentRect.height;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      dirty = true;
    });
    observer.observe(canvas);

    const styles = getComputedStyle(document.documentElement);
    const color = (name: string) => styles.getPropertyValue(name).trim();
    const empty = "rgba(255,255,255,0.05)";
    const done = color("--color-accent");
    const head = color("--color-accent-strong");

    let frame = requestAnimationFrame(function draw() {
      frame = requestAnimationFrame(draw);
      const segments = liveSegments.get(downloadId);
      if (!dirty && segments === lastDrawn) return;
      dirty = false;
      lastDrawn = segments;

      ctx.clearRect(0, 0, width, height);
      const cols = Math.max(1, Math.floor((width + GAP) / (CELL + GAP)));
      const rows = Math.max(1, Math.floor((height + GAP) / (CELL + GAP)));
      const cells = cols * rows;
      const fill = new Float32Array(cells);
      const heads = new Uint8Array(cells);
      const total = totalBytes ?? 0;

      if (complete) {
        fill.fill(1);
      } else if (segments && total > 0) {
        const perCell = total / cells;
        for (const s of segments) {
          const flushedEnd = s.start + s.downloaded;
          // Add this segment's flushed bytes to every cell it overlaps.
          let c = Math.floor(s.start / perCell);
          while (c < cells && c * perCell < flushedEnd) {
            const lo = Math.max(c * perCell, s.start);
            const hi = Math.min((c + 1) * perCell, flushedEnd);
            if (hi > lo) fill[c]! += (hi - lo) / perCell;
            c += 1;
          }
          if (flushedEnd < s.end) heads[Math.min(cells - 1, Math.floor(flushedEnd / perCell))] = 1;
        }
      }

      for (let i = 0; i < cells; i += 1) {
        const x = (i % cols) * (CELL + GAP);
        const y = Math.floor(i / cols) * (CELL + GAP);
        const f = Math.min(1, fill[i]!);
        ctx.fillStyle = empty;
        ctx.fillRect(x, y, CELL, CELL);
        if (f > 0) {
          ctx.globalAlpha = 0.25 + 0.75 * f;
          ctx.fillStyle = done;
          // Partially flushed cells fill bottom-up.
          const h = Math.max(1, CELL * f);
          ctx.fillRect(x, y + CELL - h, CELL, h);
          ctx.globalAlpha = 1;
        }
        if (heads[i]) {
          ctx.fillStyle = head;
          ctx.shadowColor = head;
          ctx.shadowBlur = 8;
          ctx.fillRect(x, y, CELL, CELL);
          ctx.shadowBlur = 0;
        }
      }
    });

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [downloadId, totalBytes, complete]);

  return <canvas ref={canvasRef} className="h-full w-full" aria-label="Segment map" role="img" />;
}
