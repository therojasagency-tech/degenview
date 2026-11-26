# Local market-data launchers

OANDA: `bash tools/launch-oanda-mac.sh` discovers accounts using the official v20 API.

OpenChart: the local bridge is tested with synthetic data. The original upstream client uses TradingView interfaces that require prior express written authorization for programmatic access. Only enable its live mode if you actually have authorization for this specific usage. Setting an environment variable does not confer permission. See https://www.tradingview.com/policies/.

The OpenChart launcher starts the pinned client and native GUI together, uses a generated local bridge token and listens on loopback only. It defaults to the original client's anonymous mode. Set DEGENVIEW_TV_LOGIN=browser to reuse OpenChart's original browser authorization method for your own authorized account. The one-time link is entered in a hidden native macOS dialog; no password or browser cookie is requested. The session remains in memory only, so reconnect after exiting. The experimental live branch adds a loopback WebSocket backed by the original client's live series. Status is forwarded but not displayed as a market-data entitlement in the GUI; delayed feeds remain delayed.

The OANDA launcher accepts an optional `--with-openchart` mode for authorized users who need both providers in one GUI. The OANDA token remains in local process environment, including the bridge subprocess in combined mode, and is not written to disk. The app must already be compiled. Close it before starting a new launcher.

Offline authorization wrapper check: `bash tools/data-bridge/run-openchart.sh ../openchart --auth-smoke`. No network or real login is used by that test.

The Rust client reconnects on failure. REST reconciles every 30 seconds while actual candle changes arrive frequently, otherwise every 5 seconds. The OANDA provider is unchanged. The live smoke uses injected upstream sockets only.

Native macOS Settings → TradingView can enable on-demand anonymous access with the pinned local checkout and installed Bun. Restart after changing the setting. The app manages the bridge through private pipes, and the child stops on stdin EOF. Existing terminal launchers still override this preference. No TradingView account credential is persisted by this feature.
