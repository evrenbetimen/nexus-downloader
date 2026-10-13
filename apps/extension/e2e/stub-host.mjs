#!/usr/bin/env node
// Stand-in for the NexusDL native messaging host: records each message to
// $NEXUSDL_STUB_LOG (one JSON per line) and answers with $NEXUSDL_STUB_REPLY
// (read from a file next to the log, since browsers don't pass env vars through).

import { appendFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dir = dirname(fileURLToPath(import.meta.url));
const config = JSON.parse(readFileSync(join(dir, "stub-config.json"), "utf8"));

let buffer = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (buffer.length >= 4) {
    const len = buffer.readUInt32LE(0);
    if (buffer.length < 4 + len) break;
    const message = JSON.parse(buffer.subarray(4, 4 + len).toString("utf8"));
    buffer = buffer.subarray(4 + len);
    appendFileSync(config.log, JSON.stringify({ message, args: process.argv.slice(2) }) + "\n");
    const body = Buffer.from(JSON.stringify(config.reply));
    const header = Buffer.alloc(4);
    header.writeUInt32LE(body.length, 0);
    process.stdout.write(Buffer.concat([header, body]));
  }
});
