// SPDX-License-Identifier: GPL-3.0-only
import assert from "node:assert/strict";
import { Effect } from "effect";
import { TRADINGVIEW } from "../server/access/integration/tradingview";
import { authorizeBrowser } from "./browser-login";

let received = "";
const method: Pick<typeof TRADINGVIEW, "authorize"> = {
  authorize: () => Effect.succeed({
    mode: "code" as const,
    url: "https://auth.tradingview.com/fixture",
    instructions: "Offline fixture",
    callback: (link: string) => Effect.sync(() => {
      received = link;
      return { type: "oauth" as const, methodID: TRADINGVIEW.method.id,
        access: JSON.stringify({ sessionid: "fixture-not-real" }), refresh: "", expires: 0 };
    }),
  }),
};
const result = await authorizeBrowser(async (url) => {
  assert.equal(url, "https://auth.tradingview.com/fixture");
  return " tradingview://browser-auth/?token=fixture ";
}, method);
assert.equal(received, "tradingview://browser-auth/?token=fixture");
assert.deepEqual(result, { sessionid: "fixture-not-real" });
await assert.rejects(() => authorizeBrowser(async () => { throw new Error("secret fixture"); }, method),
  (error: Error) => error.message === "TradingView browser sign-in failed or was cancelled.");
console.log("OpenChart browser authorization wrapper: offline smoke passed");
