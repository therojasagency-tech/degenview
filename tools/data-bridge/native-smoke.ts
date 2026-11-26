// SPDX-License-Identifier: GPL-3.0-only
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import type WebSocket from "ws";
import { encodeFrame, parseFrames } from "../server/data/providers/tradingview/protocol";
import { openChartHandler } from "./openchart-entry";

const time = Date.UTC(2026, 0, 5, 14, 30);
class FixtureSocket extends EventEmitter {
  readyState = 1;
  terminated = false;
  constructor() {
    super();
    queueMicrotask(() => this.emit("open"));
  }
  send(data: string) {
    for (const payload of parseFrames(data)) {
      const { m } = JSON.parse(payload);
      if (m === "create_series") {
        this.emit("message", Buffer.from(encodeFrame("timescale_update", [
          "cs", { sds_1: { s: [{ v: [time / 1000, 1.1, 1.2, 1.0, 1.15, null] }] } },
        ])));
      }
      if (m === "create_series" || m === "modify_series") {
        this.emit("message", Buffer.from(encodeFrame("series_completed", ["cs", "sds_1", "streaming", "s1", {}])));
      }
    }
  }
  terminate() { this.terminated = true; }
}

const token = "offline-fixture".repeat(3);
const sockets: FixtureSocket[] = [];
const handler = await openChartHandler(token, {
  fetch: async () => Response.json({ symbols: [
    { symbol: "EURUSD", description: "Euro / US Dollar", type: "forex", source_id: "OANDA", currency_code: "USD" },
    { symbol: "EURUSD", description: "Euro / US Dollar", type: "forex", source_id: "FXCM", currency_code: "USD" },
  ] }),
  websocket: () => {
    const socket = new FixtureSocket();
    sockets.push(socket);
    return socket as unknown as WebSocket;
  },
});
const server = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: handler });
try {
  const get = (path: string) => fetch(new URL(path, server.url), {
    headers: { authorization: "Bearer " + token },
  });
  const health = await get("/v1/health");
  assert.equal(health.status, 200);
  const search = await get("/v1/search?q=EURUSD");
  assert.equal(search.status, 200);
  assert.deepEqual((await search.json()).results.map((row: { id: string }) => row.id), ["OANDA:EURUSD", "FXCM:EURUSD"]);
  const query = new URLSearchParams({ symbol: "OANDA:EURUSD", resolution: "1m", from: String(time), to: String(time + 60_000) });
  const history = await get("/v1/history?" + query);
  assert.equal(history.status, 200);
  const { bars } = await history.json();
  assert.equal(bars.length, 1);
  assert.deepEqual({ ...bars[0], receivedSequence: undefined }, {
    time, open: 1.1, high: 1.2, low: 1.0, close: 1.15, volume: null, receivedSequence: undefined,
  });
  assert.equal(sockets.length, 1);
  assert.equal(sockets[0].terminated, true);
  assert.equal((await fetch(new URL("/v1/health", server.url))).status, 401);
  const rustProbe = process.env.DEGENVIEW_BRIDGE_FIXTURE_CLIENT;
  if (rustProbe) {
    const child = Bun.spawn([rustProbe], {
      env: {
        ...process.env,
        DEGENVIEW_BRIDGE_FIXTURE_PORT: String(server.port),
        DEGENVIEW_BRIDGE_FIXTURE_TOKEN: token,
      },
      stdout: "inherit",
      stderr: "inherit",
    });
    assert.equal(await child.exited, 0, "Rust bridge probe failed");
  }
  console.log("Native OpenChart client + local HTTP bridge: offline smoke passed");
} finally {
  server.stop(true);
}
