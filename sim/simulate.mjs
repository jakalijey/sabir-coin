// One-year economic simulation of SABIR. Uses the contract's formulas, in floats.
// Usage: node simulate.mjs  -> writes results.json and prints a summary.
// How people behave is an assumption made here. It is not a measurement.
import { writeFileSync } from 'node:fs';

const DAY = 86400;
export const PARAMS = {
  gate_secs: 300,
  toll_start: 1,
  entry_fee: 1,
  storage: 0.016,
  daily_sabir: 1,
  bridge_secs: 1001 * DAY,
  max_tax: 0.5,
  min_tax: 0.05,
  full_patience_secs: 40 * DAY,
  seed: 40,
};

function rng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

class Sabir {
  constructor(p) {
    Object.assign(this, { p, vault: 0, wallets: p.seed, uncollected: 0, alive: 0, ticked: 0, deposits: 0, taxes: 0, tolls: 0 });
  }
  earningNow(t) {
    return Math.min(t, this.p.bridge_secs);
  }
  tick(t) {
    const e = this.earningNow(t);
    if (e > this.ticked) {
      this.uncollected += (this.alive * (e - this.ticked) * this.p.daily_sabir) / DAY;
      this.ticked = e;
    }
  }
  supply() {
    return this.wallets + this.uncollected;
  }
  tax(age) {
    const { max_tax: a, min_tax: b, full_patience_secs: f } = this.p;
    return age >= f ? b : a - ((a - b) * age) / f;
  }
  toll(waited) {
    const { toll_start: t, gate_secs: g } = this.p;
    return waited >= g ? 0 : (t * (g - waited)) / g;
  }
  // Mirrors enter_gate + pass_gate. The walk pays no coin.
  cross(entered, waited) {
    const now = entered + waited;
    this.tick(now);
    const toll = this.toll(waited);
    const deposit = this.p.entry_fee - this.p.storage;
    this.vault += toll;
    this.tolls += toll;
    this.deposits += deposit;
    this.alive++;
    return { clock: entered, earnedTo: this.earningNow(now), deposit, paid: this.p.entry_fee + toll };
  }
  // Mirrors break_soul followed by leave with everything it earned.
  giveUp(soul, now) {
    this.tick(now);
    const sabir = ((this.earningNow(now) - soul.earnedTo) * this.p.daily_sabir) / DAY;
    this.uncollected -= sabir;
    this.wallets += sabir;
    const rate = this.tax(now - soul.clock);
    const tax = soul.deposit * rate;
    this.vault += tax;
    this.taxes += tax;
    this.deposits -= soul.deposit;
    this.alive--;
    const gross = (sabir * this.vault) / this.supply();
    const net = gross * (1 - rate);
    this.vault -= net;
    this.wallets -= sabir;
    return { back: soul.deposit - tax + net, sabir };
  }
}

const YEAR = 365;
const scenarios = {
  // Rational people: nobody gives up inside the 40 days, everyone leaves some day later.
  nobody_leaves_early: { people: 1000, earlyShare: 0 },
  some_run_early: { people: 1000, earlyShare: 0.2 },
  paper_hands: { people: 1000, earlyShare: 0.6 },
  // Half of everyone alive gives up on day 30.
  bank_run_day_30: { people: 1000, earlyShare: 0.05, run: { day: 30, share: 0.5 } },
  // A tenth of the crowd pays to skip part of the wait.
  some_pay_the_toll: { people: 1000, earlyShare: 0.05, skipShare: 0.1 },
  // The same crowd arriving in one second: nothing may change for those who wait.
  all_at_the_same_second: { people: 1000, earlyShare: 0, sameSecond: true },
  // A small crowd, the likely start.
  small_crowd: { people: 100, earlyShare: 0.1 },
};

function run(name, cfg) {
  const r = rng(name.length * 7919 + 13);
  const c = new Sabir(PARAMS);
  const people = [];
  for (let i = 0; i < cfg.people; i++) {
    const entered = cfg.sameSecond ? 0 : r() * 30 * DAY;
    const waited = r() < (cfg.skipShare || 0) ? Math.floor(r() * PARAMS.gate_secs) : PARAMS.gate_secs;
    const early = r() < cfg.earlyShare;
    let leaves = early ? entered + waited + r() * (PARAMS.full_patience_secs - waited) : entered + PARAMS.full_patience_secs + r() * (YEAR - 40) * DAY;
    if (cfg.run && entered + waited < cfg.run.day * DAY && r() < cfg.run.share) leaves = cfg.run.day * DAY;
    people.push({ entered, waited, early: leaves < entered + PARAMS.full_patience_secs, leaves });
  }
  const events = [];
  people.forEach((p) => {
    events.push([p.entered + p.waited, 0, p]);
    events.push([p.leaves, 1, p]);
  });
  events.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  for (const [t, kind, p] of events) {
    if (kind === 0) p.soul = c.cross(p.entered, p.waited);
    else Object.assign(p, c.giveUp(p.soul, t));
  }
  const mean = (l) => (l.length ? l.reduce((s, p) => s + p.back, 0) / l.length : null);
  const rounded = (v) => (v === null ? null : +v.toFixed(3));
  const early = people.filter((p) => p.early);
  const patient = people.filter((p) => !p.early).sort((a, b) => a.leaves - b.leaves);
  const q = Math.floor(patient.length / 4);
  const firstDecile = people.filter((p) => !p.early).sort((a, b) => a.entered - b.entered).slice(0, Math.floor(patient.length / 10));
  const earned = (l) => (l.length ? +(l.reduce((s, p) => s + p.sabir / ((p.leaves - p.entered - p.waited) / DAY), 0) / l.length).toFixed(3) : null);
  return {
    name,
    summary: {
      people: people.length,
      gave_up_early: early.length,
      tolls_near: +c.tolls.toFixed(2),
      taxes_near: +c.taxes.toFixed(2),
      back_early: rounded(mean(early)),
      back_first_quarter_to_leave: rounded(mean(patient.slice(0, q))),
      back_last_quarter_to_leave: rounded(mean(patient.slice(-q))),
      back_last_1_percent: rounded(mean(patient.slice(-Math.max(1, Math.floor(patient.length / 100))))),
      sabir_per_day_first_arrivals: earned(firstDecile),
      left_in_vault_near: +c.vault.toFixed(3),
    },
  };
}

const results = Object.entries(scenarios).map(([n, c]) => run(n, c));
writeFileSync(new URL('./results.json', import.meta.url), JSON.stringify({ params: PARAMS, results }, null, 1));
console.table(Object.fromEntries(results.map((r) => [r.name, r.summary])));
