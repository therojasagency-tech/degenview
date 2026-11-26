// SPDX-License-Identifier: GPL-3.0-only
// This file is staged beside the pinned OpenChart checkout by run-openchart.sh.
import { Effect } from "effect";
import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
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
  const app = process.env.DEGENVIEW_BRIDGE_APP ? resolve(process.env.DEGENVIEW_BRIDGE_APP) : undefined;
  if (app && !existsSync(app)) throw new Error("Build the DegenView app first.");
  const token = app ? randomBytes(32).toString("hex") : process.env.DEGENVIEW_BRIDGE_TOKEN;
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
  const child = app ? Bun.spawn([app], {
    env: { ...process.env, DEGENVIEW_BRIDGE_PORT: String(server.port), DEGENVIEW_BRIDGE_TOKEN: token },
    stdin: "ignore", stdout: "inherit", stderr: "inherit",
  }) : undefined;
  const stop = () => { child?.kill(); server.stop(true); };
  process.once("SIGTERM", stop);
  process.once("SIGINT", stop);
  if (child) {
    try { process.exitCode = await child.exited; } finally { server.stop(true); }
  }
}
