# Local market-data launchers

OANDA: `bash tools/launch-oanda-mac.sh` discovers accounts using the official v20 API.

OpenChart: the local bridge is tested with synthetic data. The original upstream client uses TradingView interfaces that require prior express written authorization for programmatic access. Only enable its live mode if you actually have authorization for this specific usage. Setting an environment variable does not confer permission. See https://www.tradingview.com/policies/.

The OpenChart launcher starts the pinned client and native GUI together, uses a generated local bridge token and listens on loopback only. It currently runs the client's anonymous mode; authenticated sessions and native streaming are not wired in. No account cookie is requested.

The OANDA launcher accepts an optional `--with-openchart` mode for authorized users who need both providers in one GUI. The OANDA token remains in local process environment, including the bridge subprocess in combined mode, and is not written to disk. The app must already be compiled. Close it before starting a new launcher.
