// SPDX-License-Identifier: GPL-3.0-only
// This file is staged beside the pinned OpenChart checkout by run-openchart.sh.
import { Effect } from "effect";
import { makeClient, type ClientOptions } from "../server/data/providers/tradingview/client";
import { createAdapter, createHandler } from "./adapter.mjs";

export async function openChartHandler(token: string, options: ClientOptions = {}) {
  const client = await Effect.runPromise(makeClient(options));
  return createHandler(createAdapter(client, {
    runPromise: Effect.runPromise,
    scoped: Effect.scoped,
  }), token);
}

if (import.meta.main) {
  if (process.env.DEGENVIEW_TV_AUTHORIZED !== "1") {
    throw new Error("The data provider requires permission for programmatic use.");
  }
  const token = process.env.DEGENVIEW_BRIDGE_TOKEN;
  if (!token || token.length < 32) throw new Error("A local bridge token is required.");
  const handler = await openChartHandler(token);
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    idleTimeout: 120,
    maxRequestBodySize: 1024,
    fetch: handler,
  });
  // The parent discovers the ephemeral port; credentials are never printed.
  console.log(JSON.stringify({ version: 1, port: server.port }));
  const stop = () => { server.stop(true); };
  process.once("SIGTERM", stop);
  process.once("SIGINT", stop);
}
