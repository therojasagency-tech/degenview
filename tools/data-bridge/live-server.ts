// SPDX-License-Identifier: GPL-3.0-only
// Loopback WebSocket forwards the original OpenChart live series without creating candles.
import { Deferred, Effect, Stream } from "effect";
import type { ServerWebSocket } from "bun";
import type { Client, SeriesRequest } from "../server/data/providers/tradingview/client";

const resolutions = new Set(["1s","1m","5m","15m","30m","1h","4h","1d","1W","1M"]);
const MAX_SUBSCRIPTIONS = 16;
type Connection = { series: SeriesRequest; abort: AbortController };

export function seriesRequest(url: URL): SeriesRequest | undefined {
  const symbol = url.searchParams.get("symbol");
  const resolution = url.searchParams.get("resolution");
  if (!symbol || !/^[A-Z0-9_.-]+:[^\s:]{1,150}$/i.test(symbol) || !resolution || !resolutions.has(resolution)) return;
  return { symbol, resolution: resolution as SeriesRequest["resolution"], adjustment: "raw" };
}

export function startBridgeServer(client: Client, token: string, handler: (request: Request) => Promise<Response>) {
  const active = new Set<AbortController>();
  const send = (ws: ServerWebSocket<Connection>, event: unknown) => {
    if (ws.data.abort.signal.aborted) return;
    // A full consumer must reconnect and reconcile history; never silently drop bars.
    if (ws.send(JSON.stringify(event)) <= 0) {
      ws.data.abort.abort();
      ws.close(1013, "Slow consumer");
    }
  };
  const server = Bun.serve<Connection>({
    hostname: "127.0.0.1",
    port: 0,
    idleTimeout: 120,
    maxRequestBodySize: 1024,
    fetch(request, server) {
      const url = new URL(request.url);
      if (url.pathname !== "/v1/live") return handler(request);
      if (request.method !== "GET" || request.headers.get("authorization") !== "Bearer " + token) {
        return new Response("Unauthorized", {status:401});
      }
      const series = seriesRequest(url);
      if (!series) return new Response("Invalid series", {status:400});
      if (active.size >= MAX_SUBSCRIPTIONS) return new Response("Subscription limit", {status:429});
      const abort = new AbortController();
      active.add(abort);
      if (server.upgrade(request, {data:{series,abort}})) return;
      active.delete(abort);
      return new Response("WebSocket required", {status:426});
    },
    websocket: {
      maxPayloadLength: 1024,
      backpressureLimit: 64 * 1024,
      closeOnBackpressureLimit: true,
      open(ws) {
        const {series,abort} = ws.data;
        const run = Effect.gen(function* () {
          const retired = yield* Deferred.make<void>();
          const activity = yield* client.live(series, retired);
          yield* Stream.runForEach(activity, event => Effect.sync(() => {
            send(ws, { ...event, symbol: series.symbol, resolution: series.resolution });
          }));
        }).pipe(Effect.scoped);
        void Effect.runPromise(run, {signal:abort.signal}).then(
          () => { if (!abort.signal.aborted) ws.close(1000, "Stream ended"); },
          () => { if (!abort.signal.aborted) ws.close(1011, "Provider unavailable"); },
        ).finally(() => { active.delete(abort); });
      },
      message(ws) { ws.close(1008, "Read only"); },
      close(ws) { ws.data.abort.abort(); active.delete(ws.data.abort); },
    },
  });
  return {
    port: server.port,
    url: server.url,
    stop() {
      for (const abort of active) abort.abort();
      active.clear();
      server.stop(true);
    },
  };
}
