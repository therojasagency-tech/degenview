// SPDX-License-Identifier: GPL-3.0-only
// Synthetic market data, never connected to a real exchange.
import { randomBytes } from 'node:crypto';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createAdapter, createHandler } from './adapter.mjs';

const row = { symbol: 'EURUSD_TEST', exchange: 'DEMO', name: 'Synthetic EURUSD — NOT market data', type: 'forex', currency: 'USD' };
const steps = { '1s': 1000, '1m': 60000, '5m': 300000, '15m': 900000, '30m': 1800000, '1h': 3600000, '4h': 14400000, '1d': 86400000, '1W': 604800000 };
function boundary(index, resolution) {
  if (resolution === '1M') return Date.UTC(1970, index, 1);
  return index * steps[resolution] + (resolution === '1W' ? 345600000 : 0);
}
function indexAt(time, resolution) {
  if (resolution === '1M') {
    const date = new Date(time);
    return (date.getUTCFullYear() - 1970) * 12 + date.getUTCMonth();
  }
  return Math.floor((time - (resolution === '1W' ? 345600000 : 0)) / steps[resolution]);
}
const price = n => 1.1 + Math.sin(n / 29) * 0.008 + Math.sin(n / 5) * 0.002;
export function makeFixture() {
  return {
    search(query) {
      return ['eurusd_test', 'eurusd', 'demo', 'demo:eurusd_test'].includes(query.toLowerCase()) ? [row] : [];
    },
    history({ symbol, resolution, count, from, to }) {
      if (symbol !== 'DEMO:EURUSD_TEST') return [];
      const end = Math.min(to ?? Date.now(), Date.now());
      const last = indexAt(end - 1, resolution);
      let first = last - Math.min(count ?? 5000, 5000) + 1;
      if (from !== undefined) {
        let start = indexAt(from, resolution);
        if (boundary(start, resolution) < from) start++;
        first = Math.max(first, start);
      }
      const bars = [];
      for (let n = first; n <= last; n++) {
        const open = price(n), close = price(n + 1);
        bars.push({ time: boundary(n, resolution), open, high: Math.max(open, close) + 0.0004,
          low: Math.min(open, close) - 0.0004, close, volume: null, receivedSequence: n - first + 1 });
      }
      return bars;
    },
  };
}
if (import.meta.main) {
  const app = resolve(process.argv[2] ?? 'target/DegenView.app/Contents/MacOS/DegenView');
  if (!existsSync(app)) throw new Error('Build the app with bash tools/bundle.sh first.');
  const token = randomBytes(32).toString('hex');
  const handler = createHandler(createAdapter(makeFixture(), { runPromise: async x => x, scoped: x => x }), token);
  const server = Bun.serve({ hostname: '127.0.0.1', port: 0, fetch: handler });
  const directory = mkdtempSync(join(tmpdir(), 'degenview-fixture-'));
  console.log('SYNTHETIC TEST ONLY — select TradingView (OpenChart) and search EURUSD_TEST.');
  console.log('Separate test profile:', directory);
  const child = Bun.spawn([app], {
    env: { ...process.env, DEGENVIEW_BRIDGE_PORT: String(server.port), DEGENVIEW_BRIDGE_TOKEN: token,
      DEGENVIEW_DATA_DIR: join(directory, 'data'), DEGENVIEW_CACHE_DIR: join(directory, 'cache'), DEGENVIEW_NO_KEYCHAIN: '1' },
    stdin: 'ignore', stdout: 'inherit', stderr: 'inherit',
  });
  const stop = () => { child.kill(); server.stop(true); };
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  try { process.exitCode = await child.exited; } finally { server.stop(true); }
}
