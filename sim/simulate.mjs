// 60-day economic simulation of SABIR. Uses the same formulas as the contract, in floats.
// Usage: node simulate.mjs  -> writes results.json and prints a summary.
import { writeFileSync } from 'node:fs';

const DAY = 86400;
export const PARAMS = {
  gate_secs: 300,
  toll_start: 5,
  entry_fee: 0.1,
  storage: 0.005,
  base_grant: 100000,
  max_entrants: 10000,
  max_tax: 0.5,
  min_tax: 0.05,
  full_patience_secs: 30 * DAY,
  seed: 1000,
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
    Object.assign(this, { p, vault: 0, supply: p.seed, burned: 0, entrants: 0, tolls: 0, skippers: 0, waiters: 0, exits: 0, paid: 0 });
  }
  tax(held) {
    const { max_tax: a, min_tax: b, full_patience_secs: f } = this.p;
    return held >= f ? b : a - ((a - b) * held) / f;
  }
  toll(waited) {
    const { toll_start: t, gate_secs: g } = this.p;
    return waited >= g ? 0 : (t * (g - waited)) / g;
  }
  curve() {
    return (this.p.base_grant * (this.p.max_entrants - this.entrants)) / this.p.max_entrants;
  }
  // Mirrors enter_gate + pass_gate. Returns a holder, or null if the gate is closed.
  join(now, waited) {
    if (this.entrants >= this.p.max_entrants) return null;
    const net = this.p.entry_fee - this.p.storage;
    const toll = this.toll(waited);
    this.vault += net + toll;
    this.tolls += toll;
    const cost = net + toll;
    const keep = 1 - this.tax(waited);
    const v = this.vault * keep;
    let grant = this.curve();
    if (v > cost) grant = Math.min(grant, (cost * this.supply) / (v - cost));
    this.supply += grant;
    this.entrants++;
    if (toll > 0) this.skippers++;
    else this.waiters++;
    return { bal: grant, clock: now - waited, cost };
  }
  exitValue(amount, held) {
    return ((amount * this.vault) / this.supply) * (1 - this.tax(held));
  }
  leave(h, now) {
    const pay = this.exitValue(h.bal, now - h.clock);
    this.supply -= h.bal;
    this.burned += h.bal;
    this.vault -= pay;
    this.paid += pay;
    this.exits++;
    const bal = h.bal;
    h.bal = 0;
    return { pay, bal };
  }
  backing() {
    return this.vault / this.supply;
  }
}

const scenarios = {
  // Daily newcomers, a share of them pay to skip, few leave.
  patient_crowd: { perDay: 40, skipShare: 0.1, dailyLeave: 0.005 },
  // Same crowd, but many leave within days.
  paper_hands: { perDay: 40, skipShare: 0.1, dailyLeave: 0.12 },
  // Nobody is willing to pay the toll.
  everyone_waits: { perDay: 40, skipShare: 0, dailyLeave: 0.02 },
  // Half the holders run for the exit on day 30.
  bank_run_day_30: { perDay: 40, skipShare: 0.1, dailyLeave: 0.01, run: { day: 30, share: 0.5 } },
  // A farmer opens one fresh wallet per entry on every day and exits at once.
  sybil_farm: { perDay: 40, skipShare: 0.1, dailyLeave: 0.01, sybilPerDay: 200 },
};

function run(name, cfg) {
  const r = rng(name.length * 7919 + 13);
  const c = new Sabir(PARAMS);
  const holders = [];
  const series = [];
  let sybilProfit = 0;
  let stayerFirst = null;
  for (let d = 0; d <= 60; d++) {
    const now = d * DAY;
    for (let i = 0; i < cfg.perDay; i++) {
      const skip = r() < cfg.skipShare;
      const waited = skip ? Math.floor(r() * PARAMS.gate_secs) : PARAMS.gate_secs;
      const t = now + r() * 3600;
      const h = c.join(t, waited);
      if (h) {
        holders.push(h);
        if (!stayerFirst && !skip) stayerFirst = { h, cost: h.cost };
      }
    }
    for (const h of holders) {
      if (h === stayerFirst?.h || h.bal === 0) continue;
      if (r() < cfg.dailyLeave) c.leave(h, now);
    }
    if (cfg.run && d === cfg.run.day) {
      for (const h of holders) if (h !== stayerFirst?.h && h.bal > 0 && r() < cfg.run.share) c.leave(h, now);
    }
    for (let i = 0; i < (cfg.sybilPerDay || 0); i++) {
      const h = c.join(now, PARAMS.gate_secs);
      if (!h) break;
      const { pay } = c.leave(h, now + 1);
      sybilProfit += pay - PARAMS.entry_fee;
    }
    series.push({
      day: d,
      supply: c.supply,
      vault: c.vault,
      burned: c.burned,
      backing_per_100k: c.backing() * 100000,
      entrants: c.entrants,
      holders: holders.filter((h) => h.bal > 0).length,
    });
  }
  const first = stayerFirst;
  const firstValue = first ? c.exitValue(first.h.bal, 60 * DAY - first.h.clock) : 0;
  return {
    name,
    series,
    summary: {
      entrants: c.entrants,
      skippers: c.skippers,
      waiters: c.waiters,
      exits: c.exits,
      tolls_near: +c.tolls.toFixed(2),
      vault_near: +c.vault.toFixed(2),
      paid_out_near: +c.paid.toFixed(2),
      supply: Math.round(c.supply),
      burned: Math.round(c.burned),
      first_stayer_cost_near: first ? +first.cost.toFixed(3) : 0,
      first_stayer_value_day60_near: +firstValue.toFixed(3),
      sybil_profit_near: +sybilProfit.toFixed(3),
    },
  };
}

const results = Object.entries(scenarios).map(([n, c]) => run(n, c));
writeFileSync(new URL('./results.json', import.meta.url), JSON.stringify({ params: PARAMS, results }, null, 1));
console.table(Object.fromEntries(results.map((r) => [r.name, r.summary])));
