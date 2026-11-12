// Serves the picture of a soul at /<token_id>.svg so that wallets can show it.
// It reads the soul from the chain and draws it with the engine the game itself embeds.
// Usage: CONTRACT=sabir.testnet RPC=https://rpc.testnet.fastnear.com PORT=8787 node media/server.mjs
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const CONTRACT = process.env.CONTRACT;
if (!CONTRACT) throw new Error('set CONTRACT');
const RPC = process.env.RPC || 'https://rpc.testnet.fastnear.com';
const PORT = Number(process.env.PORT || 8787);
const HOST = process.env.HOST || '127.0.0.1';
const KEEP_MS = 10 * 60 * 1000;
// A soul that does not exist yet may be minted any minute, so a miss is remembered only briefly.
const KEEP_MISS_MS = 60 * 1000;
const MAX_KEPT = 5000;
// Requests for made-up numbers must not turn into a flood of reads against the RPC.
const MAX_READS = 8;

const page = readFileSync(new URL('../web/index.html', import.meta.url), 'utf8');
const engine = page.match(/<!--SOULS--><script>([\s\S]*?)<\/script><!--\/SOULS-->/);
if (!engine) throw new Error('the drawing engine is not in web/index.html');
const sandbox = { window: {}, atob: (s) => Buffer.from(s, 'base64').toString('latin1') };
runInNewContext(engine[1], sandbox);
const SOULS = sandbox.window.SOULS;

async function view(method, args) {
  const body = JSON.stringify({
    jsonrpc: '2.0',
    id: 'soul',
    method: 'query',
    params: { request_type: 'call_function', finality: 'final', account_id: CONTRACT, method_name: method, args_base64: Buffer.from(JSON.stringify(args)).toString('base64') },
  });
  const reply = await fetch(RPC, { method: 'POST', headers: { 'content-type': 'application/json' }, body, signal: AbortSignal.timeout(8000) });
  const json = await reply.json();
  if (!json.result || !json.result.result) throw new Error('rpc');
  return JSON.parse(Buffer.from(json.result.result).toString());
}

const getSoul = (id) => view('get_soul', { token_id: id });

// How long "a day" is on this copy of the contract, in seconds.
let dayLen = null;
async function getDayLen() {
  if (dayLen === null) {
    const params = await view('get_params', {});
    dayLen = 86400 / (Number(BigInt(params.daily_sabir) / 10n ** 12n) / 1e6);
  }
  return dayLen;
}

const kept = new Map();
let reading = 0;

function stillGood(entry) {
  return entry && Date.now() - entry.at < (entry.svg ? KEEP_MS : KEEP_MISS_MS);
}

const server = createServer(async (req, res) => {
  const match = /^\/(\d{1,10})\.svg$/.exec(req.url.split('?')[0]);
  if (!match || req.method !== 'GET') {
    res.writeHead(404).end();
    return;
  }
  const id = String(Number(match[1]));
  try {
    let entry = kept.get(id);
    if (!stillGood(entry)) {
      if (reading >= MAX_READS) {
        res.writeHead(503, { 'retry-after': '5' }).end();
        return;
      }
      reading += 1;
      try {
        const soul = await getSoul(id);
        entry = { at: Date.now(), svg: soul ? SOULS.pool(SOULS.fromChain(soul, await getDayLen())) : null };
      } finally {
        reading -= 1;
      }
      if (kept.size >= MAX_KEPT) kept.clear();
      kept.set(id, entry);
    }
    if (!entry.svg) {
      res.writeHead(404, { 'cache-control': 'public, max-age=60' }).end();
      return;
    }
    res.writeHead(200, {
      'content-type': 'image/svg+xml',
      'cache-control': 'public, max-age=600',
      'access-control-allow-origin': '*',
      'content-security-policy': "default-src 'none'; style-src 'unsafe-inline'",
      'x-content-type-options': 'nosniff',
    });
    res.end(entry.svg);
  } catch (e) {
    res.writeHead(502).end();
  }
});

server.listen(PORT, HOST, () => console.log(`souls of ${CONTRACT} on ${HOST}:${PORT}`));
