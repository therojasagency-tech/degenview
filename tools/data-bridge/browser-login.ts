// SPDX-License-Identifier: GPL-3.0-only
// Reuse the original OpenChart browser authorization; keep the resulting session in memory.
import { Effect } from "effect";
import { TRADINGVIEW } from "../server/access/integration/tradingview";
import { parseSession, type Session } from "../server/data/providers/tradingview/credential";

type BrowserInteraction = (url: string, instructions: string) => Promise<string>;
type AuthorizationMethod = Pick<typeof TRADINGVIEW, "authorize">;

export async function authorizeBrowser(
  interaction: BrowserInteraction,
  method: AuthorizationMethod = TRADINGVIEW,
): Promise<Session> {
  try {
    return await Effect.runPromise(Effect.scoped(Effect.gen(function* () {
      const attempt = yield* method.authorize({});
      if (attempt.mode !== "code") return yield* Effect.fail(new Error("Unexpected login mode"));
      const url = new URL(attempt.url);
      if (url.origin !== "https://auth.tradingview.com" || url.username || url.password) {
        return yield* Effect.fail(new Error("Unexpected authorization origin"));
      }
      const link = yield* Effect.tryPromise(() => interaction(url.href, attempt.instructions));
      const credential = yield* attempt.callback(link.trim());
      const session = parseSession(credential.access);
      if (!session) return yield* Effect.fail(new Error("Invalid session"));
      return session;
    })));
  } catch {
    // Upstream exceptions may contain secret material. Never print the underlying cause.
    throw new Error("TradingView browser sign-in failed or was cancelled.");
  }
}

export async function macBrowserInteraction(url: string, instructions: string): Promise<string> {
  if (process.platform !== "darwin") throw new Error("This launcher requires macOS.");
  console.log(instructions);
  const browser = Bun.spawn(["/usr/bin/open", url], { stdout: "ignore", stderr: "ignore" });
  if (await browser.exited !== 0) throw new Error("Could not open browser");
  const dialog = Bun.spawn(["/usr/bin/osascript", "-e",
    'text returned of (display dialog "Complete the sign-in in your browser. Copy the one-time login link from TradingView and paste it here." with title "DegenView — TradingView" default answer "" with hidden answer buttons {"Cancel", "Connect"} default button "Connect" cancel button "Cancel" giving up after 300)'],
    { stdout: "pipe", stderr: "ignore" });
  const output = await new Response(dialog.stdout).text();
  if (await dialog.exited !== 0 || !output.trim()) throw new Error("Login cancelled");
  return output.trim();
}
