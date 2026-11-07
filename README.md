# SABIR

A NEAR memecoin with one mechanic: **the impatient pay the patient.**

Track: Memecoins With Mechanics. Built at NEAR HACKATHON.

## The name

*Sabır* (said "sah-bur") is Turkish for **patience**. The ticker drops the dotless ı and becomes SABIR.

Two Turkish sayings are the whole design:

- *Sabrın sonu selamettir*: "the end of patience is safety". Here the end of patience is a 5% exit tax and a share of the vault.
- *Sabreden derviş muradına ermiş*: "the dervish who waited got his wish". Here the one who waits five minutes walks through the door for free.

## The problem

Most memecoins reward whoever gets in first and sells first. Holding is a promise, and nothing in the token enforces it. SABIR puts patience in the contract: waiting is free, hurrying costs money, and that money goes to the people who stayed.

## The mechanic

1. **The Gate.** `enter_gate` costs a small fee and starts a five-minute wait. `pass_gate` is free once the wait is over. Before that it costs a toll that starts at 5 NEAR and falls linearly to zero. Grants shrink with each entrant, so waiting has a cost too. Every skip is recorded with how long the person lasted.
2. **The Vault.** Every fee and toll goes into a vault that backs the supply. `leave` burns SABIR for a share of the vault, so an exit always exists without a pool.
3. **The Exit.** `leave` and every transfer cost a tax that falls from 50% to 5% over 30 days of holding. Taxed value stays behind: on `leave` the tax remains in the vault, on a transfer the taxed tokens burn. Either way, the backing of those who stayed goes up.

Rank follows holding time: Restless, Waiting, Steady, Stone, Mountain.

## Guarantees in the contract

- NEP-141 token and NEP-145 storage. The mechanic lives in `contract/src/lib.rs`, with no backend.
- No owner field and no admin method. Parameters are set once in `new` and readable through `get_params`.
- Entering and leaving at once can never profit: `grant_for` cuts the grant so that the immediate exit value never exceeds what the entrant paid.
- A seed stays locked in the contract, so supply never reaches zero.
- Events follow NEP-297. `get_log` returns the latest 64 entries.

## Parameters

| Parameter | Value |
| --- | --- |
| Gate wait | 300 seconds |
| Toll at entry | 5 NEAR, falling to 0 |
| Entry fee | 0.1 NEAR |
| First grant | 100,000 SABIR, falling to 0 at 10,000 entrants |
| Exit tax | 50% falling to 5% |
| Full patience | 30 days |
| Locked seed | 1,000 SABIR |

## Simulation

`sim/simulate.mjs` runs the contract's formulas for 60 days under five scenarios. Run `node sim/simulate.mjs`, see the table and `sim/results.json`.

| Scenario | Entered | Paid to skip | Left | Vault (NEAR) | First patient holder: paid, value on day 60 (NEAR) | Sybil profit (NEAR) |
| --- | --- | --- | --- | --- | --- | --- |
| Patient crowd | 2,440 | 246 | 325 | 748 | 0.095, 0.428 | |
| Paper hands | 2,440 | 240 | 2,160 | 176 | 0.095, 5.721 | |
| Everyone waits | 2,440 | 0 | 1,060 | 150 | 0.095, 0.121 | |
| Bank run on day 30 | 2,440 | 246 | 1,040 | 551 | 0.095, 1.415 | |
| Sybil farm | 10,000 | 173 | 8,860 | 449 | 0.095, 2.004 | -82.4 |

The behaviour of the crowd is an assumption in the script, not a measurement.

## Limits

- **Sybil.** One person can open many accounts. Each pays the fee, and the anti-profit rule makes farming lose money in the simulation. It can still fill the gate early, which is griefing. Its only limit is its cost.
- **Patience is holding time.** A wallet with lost keys looks like a saint.
- **Winners and losers.** Stayers gain from those who leave and those who pay. Late entrants can lose if nobody follows. This is a game, not a yield product.
- **No DEX pool in v1.** A pool would pay the tax on every trade, so the vault is the exit.
- Testnet only, with no real value.

## Live on testnet

Contract: [`sabir-gate.testnet`](https://testnet.nearblocks.io/address/sabir-gate.testnet). Read it without a wallet:

```sh
near contract call-function as-read-only sabir-gate.testnet get_params json-args {} network-config testnet now
near contract call-function as-read-only sabir-gate.testnet get_log json-args '{"limit":10}' network-config testnet now
```

`scripts/testnet-flow.sh` runs the whole story against the live contract with two testnet accounts: one pays to skip, one waits, a taxed transfer, an exit.

## Try it

Run `python -m http.server 7890 --directory web` and open `http://localhost:7890`. The page reads the live contract and connects a testnet wallet (MyNearWallet, Meteor or HERE). Add `?practice` to the address to play with no wallet: the same formulas run in the browser with play money and simulated entrants.

Practice mode, step by step:

1. Open `http://localhost:7890/?practice`.
2. Press "Step up to the door". See the toll melt and the door open.
3. Press "Pay to skip". See your entry on the Wall of Impatience.
4. Press "Jump 7 days" twice. See the tax fall to 29% and the rank change to Steady.
5. Press "Send to a second wallet". See the tax burn and the clock reset to zero.

Contract tests: `cd contract && cargo test`. There are 15 unit tests.

## Background

The Gate comes from an earlier browser prototype by the same author, in which a player pays to skip a five-minute intro. That prototype has no chain code and is not part of this repository. All code here was written during the hackathon window.

## License

MIT
