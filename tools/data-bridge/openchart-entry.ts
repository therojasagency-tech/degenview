// SPDX-License-Identifier: GPL-3.0-only
// This file is staged beside the pinned OpenChart checkout by run-openchart.sh.
import { Effect } from "effect";
import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { makeClient, type ClientOptions } from "../server/data/providers/tradingview/client";
import { createAdapter, createHandler } from "./adapter.mjs";
import { authorizeBrowser, macBrowserInteraction } from "./browser-login";
import { startBridgeServer } from "./live-server";

async function openChartRuntime(token: string, options: ClientOptions = {}) {
  const client = await Effect.runPromise(makeClient(options));
  const handler = createHandler(createAdapter(client, {
    runPromise: Effect.runPromise,
    scoped: Effect.scoped,
  }), token);
  return {client,handler};
}

export async function openChartHandler(token: string, options: ClientOptions = {}) {
  return (await openChartRuntime(token, options)).handler;
}

export async function openChartServer(token: string, options: ClientOptions = {}) {
  const {client,handler} = await openChartRuntime(token, options);
  return startBridgeServer(client, token, handler);
}

if (import.meta.main) {
  if (process.env.DEGENVIEW_TV_AUTHORIZED !== "1") {
    throw new Error("The data provider requires permission for programmatic use.");
  }
  const app = process.env.DEGENVIEW_BRIDGE_APP ? resolve(process.env.DEGENVIEW_BRIDGE_APP) : undefined;
  if (app && !existsSync(app)) throw new Error("Build the DegenView app first.");
  const managed = process.env.DEGENVIEW_BRIDGE_MANAGED === "1";
  if (managed && app) throw new Error("Conflicting bridge launch modes.");
  const token = app || managed ? randomBytes(32).toString("hex") : process.env.DEGENVIEW_BRIDGE_TOKEN;
  if (!token || token.length < 32) throw new Error("A local bridge token is required.");
  const login = process.env.DEGENVIEW_TV_LOGIN ?? "anonymous";
  if (login !== "anonymous" && login !== "browser") throw new Error("Invalid TradingView login mode.");
  const session = login === "browser" ? await authorizeBrowser(macBrowserInteraction) : undefined;
  const server = await openChartServer(token, session ? {
    credential: () => Effect.succeed(session),
  } : {});
  if (managed) {
    // Only the private inherited stdout pipe carries the ephemeral loopback bearer.
    console.log(JSON.stringify({version: 1, port: server.port, token}));
    const retire = () => { server.stop(); process.exit(0); };
    process.stdin.once("end", retire);
    process.stdin.once("error", retire);
    process.stdin.resume();
  } else {
    console.log(session ? "TradingView browser session connected." : "TradingView anonymous session selected.");
    console.log(JSON.stringify({ version: 1, port: server.port }));
  }
  const child = app ? Bun.spawn([app], {
    env: { ...process.env, DEGENVIEW_BRIDGE_PORT: String(server.port), DEGENVIEW_BRIDGE_TOKEN: token },
    stdin: "ignore", stdout: "inherit", stderr: "inherit",
  }) : undefined;
  const stop = () => { child?.kill(); server.stop(); };
  process.once("SIGTERM", stop);
  process.once("SIGINT", stop);
  if (child) {
    try { process.exitCode = await child.exited; } finally { server.stop(); }
  }
}
