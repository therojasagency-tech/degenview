// SPDX-License-Identifier: GPL-3.0-only
// This file is staged beside the pinned OpenChart checkout by run-openchart.sh.
import { Effect } from "effect";
import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import { parseSession, type Session } from "../server/data/providers/tradingview/credential";
import { makeClient } from "../server/data/providers/tradingview/client";
import { authorizeBrowser, macBrowserInteraction } from "./browser-login";

import { openChartServer } from "./openchart-runtime";
import { runNativeSmoke } from "./native-smoke";
export { openChartHandler, openChartServer } from "./openchart-runtime";

if (import.meta.main && process.argv.includes("--self-test")) {
  await runNativeSmoke();
} else if (import.meta.main) {
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
  if (!["anonymous","browser","saved","enroll"].includes(login)) throw new Error("Invalid TradingView login mode.");
  if (["saved","enroll"].includes(login) && (!managed || process.stdout.isTTY || process.stdin.isTTY)) {
    throw new Error("Private managed pipes required.");
  }
  let session: Session | undefined;
  const earlyRetire = () => process.exit(0);
  if (managed) {
    // Start monitoring the parent before any network activity or dialog.
    const retire = earlyRetire;
    process.stdin.once("end", retire);
    process.stdin.once("error", retire);
  }
  if (login === "enroll") {
    process.stdin.resume();
    session = await authorizeBrowser((url, instructions) => macBrowserInteraction(url, instructions, true));
    console.log(JSON.stringify({session}));
    // Let stdout flush and exit normally; the parent closes stdin after reading.
    await new Promise<void>(() => {});
    process.exit(0);
  }
  if (login === "saved") {
    const line = await new Promise<string>((resolve,reject) => {
      const reader = createInterface({input:process.stdin, terminal:false});
      reader.once("line", value => { reader.close(); resolve(value); });
      reader.once("error", reject);
    });
    if (line.length > 32768 || !(session = parseSession(line))) throw new Error("Saved session invalid.");
    // Validate with the original client's access checks; never fall back to anonymous.
    const probe = await Effect.runPromise(makeClient({credential:()=>Effect.succeed(session)}));
    try { await Effect.runPromise(probe.access()); }
    catch { throw new Error("Saved TradingView session rejected. Reconnect in Settings."); }
  } else if (login === "browser") {
    session = await authorizeBrowser(macBrowserInteraction);
  }
  const server = await openChartServer(token, session ? {
    credential: () => Effect.succeed(session),
  } : {});
  if (managed) {
    process.stdin.removeListener("end", earlyRetire);
    process.stdin.removeListener("error", earlyRetire);
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
