// SABIR page. Live mode talks to the contract on NEAR testnet; practice mode runs the same
// formulas in the browser with play money. Add ?practice to the address for practice mode.
const CONTRACT = 'sabir-gate.testnet';
const RPCS = ['https://rpc.testnet.fastnear.com', 'https://test.rpc.fastnear.com', 'https://rpc.testnet.near.org'];
const YOCTO = 1e24;
const TOKEN = 1e18;
const P = { gate: 300, toll: 5, fee: 0.1, storage: 0.005, base: 100000, cap: 10000, maxTax: 0.5, minTax: 0.05, full: 30 * 86400, seed: 1000 };
const LIVE = !new URLSearchParams(location.search).has('practice');
const $ = (id) => document.getElementById(id);
const fmt = (n, d = 2) => Number(n).toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d });
const shortWait = (s) => (s >= 60 ? `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s` : `${Math.floor(s)}s`);
const tax = (held) => (held >= P.full ? P.minTax : P.maxTax - ((P.maxTax - P.minTax) * held) / P.full);
const tollAfter = (w) => (w >= P.gate ? 0 : (P.toll * (P.gate - w)) / P.gate);
const shortName = (a) => (a.length > 26 ? `${a.slice(0, 10)}...${a.slice(-8)}` : a);

// ---------- walls and log ----------
function push(listId, html, max) {
  const ul = $(listId);
  const li = document.createElement('li');
  li.innerHTML = html;
  ul.prepend(li);
  while (ul.children.length > max) ul.lastChild.remove();
}
const say = (t) => push('log', t, 40);
const wallSkip = (name, waited, toll) =>
  push('wallSkip', `<b>${name}</b><span>lasted ${shortWait(waited)}, paid <span class="paid num">${fmt(toll)} NEAR</span></span>`, 7);
const wallWait = (name) => push('wallWait', `<b>${name}</b><span>waited the full five minutes</span>`, 7);

// ---------- practice backend: the contract's formulas with play money ----------
function practiceBackend() {
  const chain = { vault: 0, supply: P.seed, entrants: 0 };
  const curve = () => (P.base * (P.cap - chain.entrants)) / P.cap;
  const t0 = performance.now();
  let offset = 0;
  const now = () => Math.floor((performance.now() - t0) / 1000) + offset;
  function join(waited) {
    const net = P.fee - P.storage;
    const toll = tollAfter(waited);
    chain.vault += toll;
    const cost = net + toll;
    const v = chain.vault * (1 - tax(waited));
    let grant = curve();
    if (v > cost) grant = Math.min(grant, (cost * chain.supply) / (v - cost));
    chain.supply += grant;
    chain.entrants++;
    return { grant, toll };
  }
  const exitValue = (amount, held) => ((amount * chain.vault) / chain.supply) * (1 - tax(held));
  function burnExit(amount, held) {
    const pay = exitValue(amount, held);
    chain.supply -= amount;
    chain.vault -= pay;
    return pay;
  }
  const me = { ticket: null, bal: 0, clock: 0, paid: 0 };
  let botId = 100;
  const bots = [];
  function bot(quiet) {
    const name = `practice-${botId++}.testnet`;
    const skip = Math.random() < 0.3;
    const waited = skip ? 5 + Math.floor(Math.random() * 280) : P.gate;
    chain.vault += P.fee - P.storage;
    const { grant, toll } = join(waited);
    bots.push({ bal: grant, clock: now() - waited });
    if (skip) {
      wallSkip(name, waited, toll);
      say(`${name} paid ${fmt(toll)} NEAR to skip after ${shortWait(waited)}`);
    } else {
      wallWait(name);
      say(`${name} waited and walked through`);
    }
    if (!quiet) sand.pour(skip ? 26 : 8);
    if (bots.length > 6 && Math.random() < 0.28) {
      const b = bots.splice(Math.floor(Math.random() * bots.length), 1)[0];
      say(`a practice holder left early with ${fmt(burnExit(b.bal, now() - b.clock), 3)} NEAR; the tax stayed in the vault`);
    }
  }
  for (let i = 0; i < 9; i++) bot(true);
  setInterval(() => bot(false), 7000);
  return {
    label: 'Practice mode, no real money',
    canJump: true,
    now,
    async connect() {},
    async view() {
      const held = Math.max(0, now() - me.clock);
      return {
        signedIn: true,
        account: 'you',
        ticket: me.ticket,
        balance: me.bal,
        held,
        tax: tax(held),
        exitValue: exitValue(me.bal, held),
        paid: me.paid,
      };
    },
    async enter() {
      chain.vault += P.fee - P.storage;
      me.ticket = { entered: now(), passed: false };
      me.paid = P.fee;
      say('you stepped up to the door');
    },
    async pass() {
      const waited = now() - me.ticket.entered;
      const { grant, toll } = join(waited);
      Object.assign(me.ticket, { passed: true, waited, paid: toll });
      me.bal = grant;
      me.clock = me.ticket.entered;
      me.paid += toll;
      if (toll > 0) {
        wallSkip('you', waited, toll);
        say(`you paid ${fmt(toll)} NEAR to skip after ${shortWait(waited)}`);
      } else {
        wallWait('you');
        say('you waited the full five minutes');
      }
      return toll;
    },
    async leave() {
      const held = now() - me.clock;
      const t = tax(held);
      const pay = burnExit(me.bal, held);
      say(`you left with ${fmt(pay, 3)} NEAR at ${fmt(t * 100, 1)}% tax`);
      me.bal = 0;
    },
    async send() {
      const t = tax(now() - me.clock);
      const burned = me.bal * t;
      chain.supply -= burned;
      me.bal -= burned;
      me.clock = now();
      say(`the second wallet trick burned ${fmt(burned, 0)} SABIR and reset your clock`);
      return `You paid ${fmt(t * 100, 1)}% to move your own money, and your clock went back to zero.`;
    },
    jump(days) {
      offset += days * 86400;
      const n = 3 + Math.floor(Math.random() * 4);
      for (let i = 0; i < n; i++) bot(true);
    },
  };
}

// ---------- live backend: NEAR testnet ----------
async function rpcView(method, args = {}) {
  const body = JSON.stringify({
    jsonrpc: '2.0',
    id: 'sabir',
    method: 'query',
    params: { request_type: 'call_function', finality: 'optimistic', account_id: CONTRACT, method_name: method, args_base64: btoa(JSON.stringify(args)) },
  });
  let lastErr;
  for (const url of RPCS) {
    try {
      const r = await (await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json' }, body })).json();
      if (r.result?.result) return JSON.parse(new TextDecoder().decode(new Uint8Array(r.result.result)));
      lastErr = r.error || r.result?.error;
    } catch (e) {
      lastErr = e;
    }
  }
  throw new Error(`view ${method} failed: ${JSON.stringify(lastErr)}`);
}

async function liveBackend() {
  const ver = '10.1.4';
  const [core, modalUi, mnw, meteor, here] = await Promise.all([
    import(`https://esm.sh/@near-wallet-selector/core@${ver}`),
    import(`https://esm.sh/@near-wallet-selector/modal-ui@${ver}`),
    import(`https://esm.sh/@near-wallet-selector/my-near-wallet@${ver}`),
    import(`https://esm.sh/@near-wallet-selector/meteor-wallet@${ver}`),
    import(`https://esm.sh/@near-wallet-selector/here-wallet@${ver}`),
  ]);
  const css = document.createElement('link');
  css.rel = 'stylesheet';
  css.href = `https://esm.sh/@near-wallet-selector/modal-ui@${ver}/styles.css`;
  document.head.append(css);
  const selector = await core.setupWalletSelector({
    network: 'testnet',
    modules: [mnw.setupMyNearWallet(), meteor.setupMeteorWallet(), here.setupHereWallet()],
  });
  const modal = modalUi.setupModal(selector, { contractId: CONTRACT });
  let chainOffset = 0;
  const now = () => Math.floor(Date.now() / 1000) + chainOffset;
  const account = () => selector.store.getState().accounts[0]?.accountId;
  selector.store.observable.subscribe(() => render());

  async function tx(method, args, depositYocto) {
    const wallet = await selector.wallet();
    return wallet.signAndSendTransaction({
      receiverId: CONTRACT,
      actions: [{ type: 'FunctionCall', params: { methodName: method, args, gas: '100000000000000', deposit: String(depositYocto) } }],
    });
  }
  const yocto = (near) => BigInt(Math.round(near * 1e6)) * 10n ** 18n;

  const seen = new Set();
  async function pollLog(quiet) {
    const entries = (await rpcView('get_log', { limit: 30 })).reverse();
    for (const e of entries) {
      const key = `${e.kind}|${e.who}|${e.at}|${e.tokens}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const who = shortName(e.who);
      const near = Number(e.near) / YOCTO;
      if (e.kind === 'skip') {
        wallSkip(who, e.detail, near);
        say(`${who} paid ${fmt(near)} NEAR to skip after ${shortWait(e.detail)}`);
        if (!quiet) sand.pour(26);
      } else if (e.kind === 'wait') {
        wallWait(who);
        say(`${who} waited and walked through`);
        if (!quiet) sand.pour(10);
      } else if (e.kind === 'enter') {
        say(`${who} stepped up to the door`);
      } else if (e.kind === 'leave') {
        say(`${who} left with ${fmt(near, 3)} NEAR at ${fmt(e.detail / 100, 1)}% tax`);
        if (!quiet) sand.burst(0.15);
      } else if (e.kind === 'tax') {
        say(`${who} paid ${fmt(e.detail / 100, 1)}% tax to move SABIR; ${fmt(Number(e.tokens) / TOKEN, 0)} burned`);
      }
    }
  }
  const st = await rpcView('get_state');
  chainOffset = st.now - Math.floor(Date.now() / 1000);
  await pollLog(true);
  setInterval(() => pollLog(false).catch(() => {}), 6000);
  $('logNote').textContent = `Read from ${CONTRACT} on NEAR testnet. Every entry is on chain.`;

  return {
    label: `Live on NEAR testnet: ${CONTRACT}`,
    canJump: false,
    now,
    async connect() {
      modal.show();
    },
    async view() {
      const id = account();
      if (!id) return { signedIn: false };
      const h = await rpcView('get_holder', { account_id: id });
      const t = h.ticket;
      return {
        signedIn: true,
        account: id,
        ticket: t && { entered: t.entered, passed: t.passed, waited: t.waited_secs, paid: Number(t.paid) / YOCTO },
        balance: Number(h.balance) / TOKEN,
        rawBalance: h.balance,
        held: h.held_secs,
        tax: h.tax_bps / 10000,
        rank: h.rank,
        exitValue: Number(h.exit_value) / YOCTO,
        paid: t ? P.fee + Number(t.paid) / YOCTO : 0,
        tollNow: Number(h.toll_now) / YOCTO,
      };
    },
    async enter() {
      await tx('enter_gate', {}, yocto(P.fee));
    },
    async pass(view) {
      // The toll only falls while the transaction is in flight, so the current toll is enough.
      const toll = tollAfter(now() - view.ticket.entered);
      await tx('pass_gate', {}, toll > 0 ? yocto(toll + 0.01) : 0n);
      return toll;
    },
    async leave(view) {
      await tx('leave', { amount: view.rawBalance }, 0n);
    },
    async send(view) {
      const to = prompt('Send all your SABIR to which testnet account? It must be registered with this token.');
      if (!to) return '';
      await tx('ft_transfer', { receiver_id: to.trim(), amount: view.rawBalance }, 1n);
      return 'Sent. The tax burned on the way.';
    },
    jump() {},
  };
}

// ---------- page ----------
let api;
let current = { signedIn: false };
let busy = false;
function action(label, cls, fn) {
  const b = document.createElement('button');
  b.className = `btn ${cls}`;
  b.textContent = label;
  b.onclick = async () => {
    if (busy) return;
    busy = true;
    b.disabled = true;
    try {
      await fn();
    } catch (e) {
      $('hint').textContent = `That did not go through: ${e.message || e}`;
    }
    busy = false;
    await render();
  };
  $('actions').append(b);
  return b;
}
function setDoor(p) {
  document.documentElement.style.setProperty('--open', p);
  document.documentElement.style.setProperty('--glow', 0.05 + p * 0.5);
}
let skipBtn = null;
let lastState = '';
async function render() {
  if (!api) return;
  try {
    current = await api.view();
  } catch (e) {
    $('hint').textContent = 'The chain did not answer. Trying again.';
    return;
  }
  const v = current;
  const state = !v.signedIn ? 'signin' : !v.ticket ? 'outside' : !v.ticket.passed ? 'waiting' : 'through';
  const changed = state !== lastState;
  if (changed && lastState === 'waiting' && state === 'through') {
    if (v.ticket.paid > 0) sand.burst(1);
    else sand.pour(60);
  }
  lastState = state;
  $('actions').innerHTML = '';
  skipBtn = null;
  $('tollBox').hidden = state !== 'waiting';
  $('meStats').hidden = state !== 'through';
  if (state === 'signin') {
    setDoor(0);
    $('title').textContent = 'Can you wait five minutes?';
    $('lead').textContent = 'There is a door. It opens for free if you wait. Or pay to walk through right now, and your money goes to the people who waited.';
    action('Connect a testnet wallet', 'main', () => api.connect());
    const a = document.createElement('a');
    a.href = '?practice';
    a.className = 'btn ghost';
    a.textContent = 'Try it with play money';
    a.style.textDecoration = 'none';
    $('actions').append(a);
    $('hint').textContent = 'Testnet only. No real money.';
  } else if (state === 'outside') {
    setDoor(0);
    $('title').textContent = 'Can you wait five minutes?';
    $('lead').textContent = 'There is a door. It opens for free if you wait. Or pay to walk through right now, and your money goes to the people who waited.';
    action('Step up to the door (0.1 NEAR)', 'main', async () => {
      await api.enter();
      sand.pour(10);
    });
    if (changed) $('hint').textContent = LIVE ? `Signed in as ${shortName(v.account)}.` : 'Practice mode: play money, the same formulas as the contract.';
  } else if (state === 'waiting') {
    $('title').textContent = 'The door is opening.';
    $('lead').textContent = 'Slowly. That is the point. Every second makes the toll cheaper. Are you sure you need to be in there right now?';
    skipBtn = action('Pay to skip', 'main', () => api.pass(current));
    if (changed) $('hint').textContent = 'Paying is allowed. It goes on the wall.';
    updateWaiting();
  } else {
    setDoor(1);
    $('title').textContent = v.balance > 0 ? 'Now stay.' : 'You left.';
    $('lead').textContent = v.balance > 0
      ? 'Your tax falls every day you hold. Every person who leaves before you makes your share larger.'
      : 'Your tax is still in the vault, working for the people who stayed.';
    if (v.balance > 0) {
      if (api.canJump) action('Jump 7 days', 'ghost', async () => {
        api.jump(7);
        sand.pour(40);
      });
      action('Leave through the vault', 'main', async () => {
        await api.leave(current);
        sand.burst();
      });
      action('Send to a second wallet', 'ghost', async () => {
        const msg = await api.send(current);
        if (msg) {
          sand.burst(0.35);
          $('hint').textContent = msg;
        }
      });
    }
    if (changed && v.ticket.paid > 0) $('hint').textContent = 'Your name is on the Wall of Impatience. Your toll now backs everyone else.';
    else if (changed) $('hint').textContent = 'You did not pay a thing. Someone else did.';
    updateThrough();
  }
}
function updateWaiting() {
  const w = Math.max(0, api.now() - current.ticket.entered);
  const t = tollAfter(w);
  const p = Math.min(1, w / P.gate);
  $('toll').textContent = fmt(t);
  $('waitBar').style.width = `${p * 100}%`;
  $('clock').textContent = `${shortWait(w)} waited, ${shortWait(Math.max(0, P.gate - w))} to go. Your grant shrinks a little with each person who gets in first.`;
  setDoor(p * 0.92);
  if (skipBtn && !busy) skipBtn.textContent = t > 0 ? `Pay ${fmt(t)} NEAR to skip` : 'Walk through, free';
}
function updateThrough() {
  const v = current;
  const held = v.held;
  const rank = v.rank || (held / P.full < 0.1 ? 'Restless' : held / P.full < 0.35 ? 'Waiting' : held / P.full < 0.7 ? 'Steady' : held < P.full ? 'Stone' : 'Mountain');
  $('sBal').textContent = fmt(v.balance, 0);
  $('sRank').textContent = v.balance > 0 ? rank : 'Gone';
  $('sTax').textContent = `${fmt(v.tax * 100, 1)}%`;
  $('sVal').textContent = fmt(v.exitValue, 3);
  $('sHeld').textContent = fmt(held / 86400, 1);
  $('sCost').textContent = fmt(v.paid, 2);
}

// ---------- sand: light physics, grains fall and pile up at the foot of the door ----------
const sand = (() => {
  const cv = $('sand');
  const g = cv.getContext('2d');
  const reduce = matchMedia('(prefers-reduced-motion:reduce)').matches;
  let W = 0, H = 0, cols = [], dpr = 1;
  const grains = [];
  const CELL = 5;
  function size() {
    dpr = Math.min(devicePixelRatio || 1, 2);
    W = cv.clientWidth;
    H = cv.clientHeight;
    cv.width = W * dpr;
    cv.height = H * dpr;
    cols = new Array(Math.ceil(W / CELL) + 1).fill(0);
  }
  addEventListener('resize', size);
  size();
  function spawn(x, y, vx, vy) {
    if (grains.length < 900) grains.push({ x, y, vx, vy });
  }
  function pour(n) {
    if (reduce) return;
    for (let i = 0; i < n; i++) spawn(W / 2 + (Math.random() - 0.5) * W * 0.34, -10 - Math.random() * 70, (Math.random() - 0.5) * 0.6, Math.random() * 1.5);
  }
  function burst(share = 1) {
    if (reduce) return;
    for (let c = 0; c < cols.length; c++) {
      const take = Math.floor(cols[c] * share);
      cols[c] -= take;
      for (let k = 0; k < Math.min(take, 3); k++) spawn(c * CELL, H - cols[c] * CELL, (Math.random() - 0.5) * 8, -4 - Math.random() * 9);
    }
  }
  function settle(c) {
    // Sand slides down when a column towers over its neighbour.
    for (let s = 0; s < 4; s++) {
      const l = c > 0 ? cols[c - 1] : Infinity;
      const r = c < cols.length - 1 ? cols[c + 1] : Infinity;
      if (cols[c] - l > 1 && l <= r) { cols[c]--; cols[--c]++; }
      else if (cols[c] - r > 1) { cols[c]--; cols[++c]++; }
      else break;
    }
  }
  function frame() {
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);
    g.fillStyle = '#D97706';
    for (let c = 0; c < cols.length; c++) if (cols[c]) g.fillRect(c * CELL, H - cols[c] * CELL, CELL, cols[c] * CELL);
    g.fillStyle = '#F0A63C';
    for (let i = grains.length - 1; i >= 0; i--) {
      const p = grains[i];
      p.vy += 0.22;
      p.x += p.vx;
      p.y += p.vy;
      const c = Math.max(0, Math.min(cols.length - 1, Math.floor(p.x / CELL)));
      if (p.y >= H - cols[c] * CELL) {
        if (cols[c] * CELL < H * 0.32) cols[c]++;
        settle(c);
        grains.splice(i, 1);
      } else {
        g.fillRect(p.x, p.y, 3, 3);
      }
    }
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
  return { pour, burst };
})();
$('doorSide').addEventListener('pointerdown', () => sand.pour(6));

// ---------- simulation chart ----------
const LABEL = { patient_crowd: 'Patient crowd', paper_hands: 'Paper hands', everyone_waits: 'Everyone waits', bank_run_day_30: 'Bank run, day 30', sybil_farm: 'Sybil farm' };
function drawChart(active) {
  const data = window.SIM.results;
  const [L, R, T, B] = [46, 12, 12, 30];
  const W = 720, H = 340;
  const max = Math.max(...data.flatMap((r) => r.series.map((s) => s.backing_per_100k)));
  const x = (d) => L + (d / 60) * (W - L - R);
  const y = (v) => H - B - (v / max) * (H - T - B);
  let out = '';
  for (let i = 0; i <= 4; i++) {
    const v = (max * i) / 4;
    out += `<line x1="${L}" x2="${W - R}" y1="${y(v)}" y2="${y(v)}" stroke="rgba(232,224,208,.14)"/><text x="${L - 8}" y="${y(v) + 4}" fill="rgba(245,239,225,.62)" font-size="12" text-anchor="end">${v.toFixed(v < 10 ? 1 : 0)}</text>`;
  }
  for (const d of [0, 15, 30, 45, 60]) out += `<text x="${x(d)}" y="${H - 8}" fill="rgba(245,239,225,.62)" font-size="12" text-anchor="middle">day ${d}</text>`;
  for (const r of data) {
    const on = r.name === active;
    const pts = r.series.map((s) => `${x(s.day).toFixed(1)},${y(s.backing_per_100k).toFixed(1)}`).join(' ');
    out += `<polyline points="${pts}" fill="none" stroke="${on ? '#D97706' : 'rgba(245,239,225,.2)'}" stroke-width="${on ? 3 : 1.5}"/>`;
  }
  $('chart').innerHTML = out;
  const s = data.find((r) => r.name === active).summary;
  const row = (a, b) => `<tr><td class="dim">${a}</td><td class="num">${b}</td></tr>`;
  $('facts').innerHTML =
    row('Entered the gate', fmt(s.entrants, 0)) +
    row('Paid to skip', fmt(s.skippers, 0)) +
    row('Left early', fmt(s.exits, 0)) +
    row('Tolls paid', `${fmt(s.tolls_near, 0)} NEAR`) +
    row('Vault after 60 days', `${fmt(s.vault_near, 0)} NEAR`) +
    row('SABIR burned', fmt(s.burned, 0)) +
    row('First patient holder paid', `${fmt(s.first_stayer_cost_near, 3)} NEAR`) +
    row('Their exit value, day 60', `${fmt(s.first_stayer_value_day60_near, 3)} NEAR`) +
    (active === 'sybil_farm' ? row('Sybil farmer profit', `${fmt(s.sybil_profit_near, 1)} NEAR`) : '');
  document.querySelectorAll('.tab').forEach((t) => t.classList.toggle('on', t.dataset.k === active));
}
for (const r of window.SIM.results) {
  const b = document.createElement('button');
  b.className = 'tab';
  b.dataset.k = r.name;
  b.textContent = LABEL[r.name];
  b.onclick = () => drawChart(r.name);
  $('tabs').append(b);
}
drawChart('paper_hands');

// ---------- start ----------
(async () => {
  sand.pour(90);
  if (LIVE) {
    try {
      api = await liveBackend();
    } catch (e) {
      api = practiceBackend();
      $('hint').textContent = 'The live contract could not be reached, so this is practice mode.';
    }
  } else {
    api = practiceBackend();
  }
  $('mode').textContent = api.label;
  await render();
  setInterval(() => {
    if (lastState === 'waiting') updateWaiting();
    if (lastState === 'through' && !LIVE && !busy) api.view().then((v) => { current = v; updateThrough(); });
  }, 500);
  if (LIVE) setInterval(() => !busy && render(), 8000);
})();
