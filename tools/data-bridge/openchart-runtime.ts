// SPDX-License-Identifier: GPL-3.0-only
import { Effect } from "effect";
import { makeClient, type ClientOptions } from "../server/data/providers/tradingview/client";
import { createAdapter, createHandler } from "./adapter.mjs";
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

