// SPDX-License-Identifier: GPL-3.0-only
// Local bridge contract for an authorized OpenChart client; no provider protocol here.
const resolutions = new Set(['1s', '1m', '5m', '15m', '30m', '1h', '4h', '1d', '1W', '1M']);
const adjustments = new Set(['raw', 'split', 'split_dividend']);

export class InvalidQuery extends Error {}

function integer(params, name, minimum, maximum) {
  const value = params.get(name);
  if (value === null) return undefined;
  if (!/^\d+$/.test(value)) throw new InvalidQuery(name);
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < minimum || number > maximum) {
    throw new InvalidQuery(name);
  }
  return number;
}

// The native client owns validation, paging, credentials, rate limits and I/O.
export function createAdapter(client, { runPromise, scoped }) {
  return {
    async search(params) {
      const query = params.get('q')?.trim();
      if (!query || query.length > 200) throw new InvalidQuery('q');
      const limit = integer(params, 'limit', 1, 100) ?? 50;
      const rows = await runPromise(client.search(query));
      return rows.slice(0, limit).map(row => ({
        ...row, id: row.exchange + ':' + row.symbol,
      }));
    },
    async history(params) {
      const symbol = params.get('symbol');
      const resolution = params.get('resolution');
      const adjustment = params.get('adjustment') ?? 'raw';
      if (!symbol || !/^[A-Z0-9_.-]+:[^\s:]{1,150}$/i.test(symbol)) throw new InvalidQuery('symbol');
      if (!resolutions.has(resolution)) throw new InvalidQuery('resolution');
      if (!adjustments.has(adjustment)) throw new InvalidQuery('adjustment');
      const count = integer(params, 'count', 1, 5000);
      const from = integer(params, 'from', 0, Number.MAX_SAFE_INTEGER);
      const to = integer(params, 'to', 1, Number.MAX_SAFE_INTEGER);
      if (count === undefined && from === undefined) throw new InvalidQuery('count or from');
      if (from !== undefined && to !== undefined && from >= to) throw new InvalidQuery('range');
      const bars = await runPromise(scoped(client.history({
        symbol, resolution, adjustment,
        ...(count === undefined ? {} : { count }),
        ...(from === undefined ? {} : { from }),
        ...(to === undefined ? {} : { to }),
      })));
      return { provider: 'tradingview', symbol, resolution, adjustment, bars };
    },
  };
}

// HTTP transport is deliberately read-only and requires a local bearer token.
export function createHandler(adapter, token) {
  if (typeof token !== 'string' || token.length < 32) throw new Error('A local bridge token is required');
  return async request => {
    const response = (data, status = 200) => Response.json(data, {
      status, headers: { 'cache-control': 'no-store' },
    });
    if (request.headers.get('authorization') !== 'Bearer ' + token) return response({ error: 'unauthorized' }, 401);
    if (request.method !== 'GET') return response({ error: 'method_not_allowed' }, 405);
    const url = new URL(request.url);
    try {
      switch (url.pathname) {
        case '/v1/health': return response({ version: 1, provider: 'tradingview', capabilities: ['search', 'history'] });
        case '/v1/search': return response({ provider: 'tradingview', results: await adapter.search(url.searchParams) });
        case '/v1/history': return response(await adapter.history(url.searchParams));
        default: return response({ error: 'not_found' }, 404);
      }
    } catch (error) {
      if (error instanceof InvalidQuery) return response({ error: 'invalid_query', field: error.message }, 400);
      // Upstream error details may contain credentials; do not return or log them.
      return response({ error: 'provider_unavailable' }, 502);
    }
  };
}
