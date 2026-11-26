import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createAdapter, createHandler } from './adapter.mjs';
const token = 'x'.repeat(32);
const bar = { time: 1700000000000, open: 1.1, high: 1.2, low: 1.0, close: 1.15, volume: null, receivedSequence: 7 };
function fixture() {
  const calls = [];
  const client = {
    search: q => { calls.push(q); return [{symbol:'EURUSD',exchange:'OANDA'},{symbol:'EURUSD',exchange:'FXCM'}]; },
    history: req => { calls.push(req); return [bar]; },
  };
  const handler = createHandler(createAdapter(client, {runPromise: async x => x, scoped: x => x}), token);
  const request = (path, auth = token, method = 'GET') => handler(new Request('http://localhost'+path,{method,headers:{authorization:'Bearer '+auth}}));
  return {calls, request};
}
test('search retains venue identities', async () => {
  const {request} = fixture();
  assert.deepEqual((await (await request('/v1/search?q=EURUSD')).json()).results.map(r=>r.id), ['OANDA:EURUSD','FXCM:EURUSD']);
});
test('history forwards exact resolution, range and nullable volume', async () => {
  const {request,calls} = fixture();
  const result = await request('/v1/history?symbol=OANDA%3AEURUSD&resolution=1h&from=1700000000000&to=1700003600000');
  assert.equal(result.status,200);
  assert.deepEqual(calls[0],{symbol:'OANDA:EURUSD',resolution:'1h',adjustment:'raw',from:1700000000000,to:1700003600000});
  assert.deepEqual((await result.json()).bars,[bar]);
});
test('unsupported resolution and reversed range fail before contacting client', async () => {
  const {request,calls} = fixture();
  assert.equal((await request('/v1/history?symbol=OANDA:EURUSD&resolution=3h&count=5')).status,400);
  assert.equal((await request('/v1/history?symbol=OANDA:EURUSD&resolution=1h&from=10&to=9')).status,400);
  assert.equal(calls.length,0);
});
test('token and read-only method enforced', async () => {
  const {request,calls} = fixture();
  assert.equal((await request('/v1/search?q=EURUSD','wrong')).status,401);
  assert.equal((await request('/v1/search?q=EURUSD',token,'POST')).status,405);
  assert.equal(calls.length,0);
});
test('upstream error does not leak sensitive detail', async () => {
  const handler = createHandler({search: async()=>{throw new Error('secret');}},token);
  const result = await handler(new Request('http://localhost/v1/search?q=x',{headers:{authorization:'Bearer '+token}}));
  assert.equal(result.status,502);
  assert.deepEqual(await result.json(),{error:'provider_unavailable'});
});
