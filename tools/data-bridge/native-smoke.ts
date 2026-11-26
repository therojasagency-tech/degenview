// SPDX-License-Identifier: GPL-3.0-only
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import WebSocket from "ws";
import { encodeFrame, parseFrames } from "../server/data/providers/tradingview/protocol";
import { openChartServer } from "./openchart-runtime";

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

export async function runNativeSmoke() {
const token = "offline-fixture".repeat(3);
const sockets: FixtureSocket[] = [];
const server = await openChartServer(token, {
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
  const liveUrl = new URL("/v1/live?" + new URLSearchParams({symbol:"OANDA:EURUSD",resolution:"1m"}), server.url);
  liveUrl.protocol = "ws:";
  const events: any[] = [];
  const live = new WebSocket(liveUrl, {headers:{authorization:"Bearer " + token}});
  try {
    await new Promise<void>((resolve,reject) => {
      const timeout = setTimeout(() => reject(new Error("Live smoke timeout")), 8000);
      const finish = (error?: Error) => { clearTimeout(timeout); error ? reject(error) : resolve(); };
      live.on("error", finish);
      live.on("close", () => finish(new Error("Live closed before fixture")));
      live.on("message", data => {
        const event = JSON.parse(data.toString());
        events.push(event);
        if (events.some(e => e.type === "bars") && events.some(e => e.type === "status")) finish();
      });
    });
    assert.equal(events.find(e=>e.type==="bars").bars[0].close,1.15);
    assert.equal(events.find(e=>e.type==="status").status.mode,"live");
    assert.ok(events.every(e=>e.symbol==="OANDA:EURUSD" && e.resolution==="1m"));
  } finally {
    live.close();
  }
  const deadline = Date.now()+3000;
  while (!sockets.at(-1)?.terminated && Date.now()<deadline) await Bun.sleep(20);
  assert.equal(sockets.at(-1)?.terminated,true,"local disconnect must release upstream socket");
  assert.equal((await fetch(new URL("/v1/live?symbol=OANDA:EURUSD&resolution=1m",server.url))).status,401);
  console.log("Native OpenChart client + local WebSocket: offline smoke passed");
  console.log("Native OpenChart client + local HTTP bridge: offline smoke passed");
} finally {
  server.stop();
}

}

if (import.meta.main) await runNativeSmoke();
