import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeFixture } from './gui-fixture.mjs';
test('fixture cannot impersonate real market data', () => {
  const fixture = makeFixture();
  assert.equal(fixture.search('EURUSD_TEST')[0].exchange, 'DEMO');
  assert.deepEqual(fixture.history({ symbol: 'OANDA:EURUSD', resolution: '1m', count: 5 }), []);
});
test('fixture returns native candles in range and preserves null volume', () => {
  const fixture = makeFixture();
  const from = Date.UTC(2026, 0, 1), to = from + 3 * 60000;
  const bars = fixture.history({ symbol: 'DEMO:EURUSD_TEST', resolution: '1m', from, to });
  assert.deepEqual(bars.map(b => b.time), [from, from+60000, from+120000]);
  assert.ok(bars.every(b => b.volume === null && b.high >= Math.max(b.open,b.close) && b.low <= Math.min(b.open,b.close)));
});
test('fixture calendar months and Mondays are aligned', () => {
  const fixture = makeFixture();
  const to = Date.UTC(2026, 2, 1);
  const args = { symbol: 'DEMO:EURUSD_TEST', to, count: 2 };
  assert.deepEqual(fixture.history({...args,resolution:'1M'}).map(b=>b.time),[Date.UTC(2026,0,1),Date.UTC(2026,1,1)]);
  assert.ok(fixture.history({...args,resolution:'1W'}).every(b=>new Date(b.time).getUTCDay()===1));
});
