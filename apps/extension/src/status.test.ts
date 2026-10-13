import { expect, it } from "vitest";

import { describe as describeStatus } from "./status";

it("explains each connection state", () => {
  expect(describeStatus({ ok: true, version: "0.2.0", appRunning: true })).toEqual({
    state: "connected",
    text: "Connected to NexusDL 0.2.0.",
  });
  expect(describeStatus({ ok: true, version: "0.2.0", appRunning: false }).state).toBe("installed");
  expect(describeStatus(new Error("Specified native messaging host not found.")).state).toBe("missing");
  expect(describeStatus({ ok: false, error: "x" }).state).toBe("missing");
});
