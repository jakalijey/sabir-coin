# SABIR

A NEAR memecoin only patience can earn. A five-minute walk draws your soul, an NFT that keeps your deposit. From then on every soul earns one SABIR a day, and whoever leaves first pays whoever leaves last.

Tracks: Memecoins With Mechanics, NFTs That Do Something, Games With Real Economies. Built at NEAR HACKATHON.

## The name

*Sabır* (said "sah-bur") is Turkish for **patience**. The ticker drops the dotless ı and becomes SABIR.

- *Sabrın sonu selamettir*: "the end of patience is safety". Here the end of patience is the lowest exit tax and a share of the vault.
- *Sabreden derviş muradına ermiş*: "the dervish who waited got his wish". Here the one who waits crosses for free.

## The problem

Most memecoins pay whoever arrives first. Holding is a promise that nothing enforces, and a newcomer's money goes to the people who came before. SABIR takes arrival order out of the contract: only time earns, the money you put in stays yours, and the only money anyone can win is what the impatient leave behind.

## The mechanic

1. **The Walk.** `enter_gate` takes 1 NEAR and starts five minutes on a bridge. The game reads how you behave. `pass_gate` is free once the wait is over; before that it costs a toll that melts from 1 NEAR to zero and goes to the vault. The walk pays no coin. How long you lasted decides the layer you are born on: Chaos, Middle, Threshold or Builder.
2. **The Soul.** Crossing mints a NEP-171 token drawn from the chain's random seed and your walk. It holds your deposit (0.984 NEAR; 0.016 pays for storage) and earns **1 SABIR per day** from that moment. The rate is the same for every soul. `collect` moves what it earned to your wallet. The picture changes as the soul ages: at 1, 7, 40, 100, 365 and 1,001 days.
3. **The Vault.** It holds tolls, exit taxes, 2% of every soul sale and anything people choose to add with `feed_vault`. Entry money never goes in, so a newcomer never pays the people who came earlier. `leave` burns SABIR for a share of the vault.
4. **The Exit.** `break_soul` burns the NFT and returns the deposit minus a tax that falls from 50% to 5% over 40 days of the soul's age. It never reaches zero. The tax stays in the vault, so everyone who leaves pays those still there. `leave` and transfers cost the same tax on the coin's own age; a `ft_transfer_call` pays it only on what the receiver really keeps.
5. **The Market.** `list_soul` and `buy_soul` sell a soul inside the contract: 1% to the creator, 2% to the vault, the rest to the seller, all at the moment of sale. The deposit goes with the soul. What it earned so far stays with the seller. Every sale halves its age, so the last forms of a soul cannot be bought. `buy_soul` takes the price the buyer saw, so a seller cannot change it under them. This is the only way a soul changes hands: `nft_transfer` and `nft_transfer_call` are closed, so a free transfer cannot stand in for a sale. The cut is taken from the listed price, and a soul cannot be listed for less than breaking it returns at that moment, so the cut is always paid on at least what the soul holds. A listing keeps its price while the soul ages; if breaking it later returns more, a stranger can buy it for less than it holds.
6. **The Exit Book.** `break_soul` takes a reason. `get_exit_book` returns, for anyone, how many souls were broken, on which day, for which reason, and from which layer. The five-minute reading of a person is checked against what they really did with money at stake.

## What the numbers mean

| Number | Meaning |
| --- | --- |
| 1 SABIR | One day a soul has waited. Supply is the sum of those days. |
| 40 days | *Çile*, the traditional forty-day ordeal. The tax reaches its floor. |
| 1,001 days | The length of the Mevlevi novice's ordeal. The bridge closes: nobody can enter and no soul earns any more. A soul can earn at most 1,001 SABIR. |

## Guarantees in the contract

- NEP-141, NEP-145, NEP-171, NEP-177 and NEP-181, with NFT transfers closed on purpose. The mechanic lives in `contract/src`, with no backend.
- No owner field. Parameters are set once in `new` and readable through `get_params`. `creator` receives its cut of sales and can do one other thing: `set_media_base`, which moves the address pictures are served from, so that they do not die with a domain name. It touches no money and no rule.
- A soul's earnings depend only on the seconds it has lived. Arrival order, the size of the crowd and paying the toll change nothing.
- The vault and the deposits are separate books. Paying out of one can never touch the other. A payment that cannot be delivered, because the receiving account no longer exists, goes to the vault.
- `leave` counts every SABIR earned so far, collected or not, so collecting early cannot take a larger share.
- A soul changes hands only through `buy_soul`, which registers the buyer, so its earnings can always be paid. Listing and selling cannot strand a soul or its deposit.
- Events follow NEP-297. `get_log` returns the latest 64 entries.

## Parameters

| Parameter | Value |
| --- | --- |
| Entry | 1 NEAR, of which 0.984 stays inside the soul |
| Walk | 300 seconds |
| Toll | 1 NEAR, melting to 0 over the walk |
| Earning | 1 SABIR per soul per day |
| Bridge | Open for 1,001 days |
| Tax | 50% falling to 5% over 40 days |
| Soul sale | 1% creator, 2% vault |
| Locked seed | 40 SABIR |

## Simulation

`node sim/simulate.mjs` runs the contract's formulas for one year: 1,000 people arrive over 30 days and leave on random days, except where noted. The figures are the average NEAR each person gets back for the 1 NEAR they put in, breaking the soul and burning all it earned. The full table is in `sim/results.json`.

| Scenario | Gave up inside 40 days | Back: gave up early | Back: first quarter to leave | Back: last quarter to leave | Back: last 1% |
| --- | --- | --- | --- | --- | --- |
| Nobody leaves early | 0 | | 0.941 | 1.054 | 1.229 |
| Some run early | 235 | 0.735 | 1.016 | 1.135 | 1.324 |
| Paper hands | 616 | 0.819 | 1.200 | 1.334 | 1.551 |
| Bank run on day 30 | 538 | 0.751 | 1.203 | 1.344 | 1.559 |
| A tenth pay the toll | 40 | 0.745 | 1.000 | 1.119 | 1.304 |
| All in the same second | 0 | | 0.941 | 1.054 | 1.232 |
| 100 people | 6 | 0.693 | 0.960 | 1.074 | 1.215 |

The behaviour of the crowd is an assumption in the script, not a measurement. Measuring it is what the Exit Book is for.

## Limits

- **Winners and losers.** If nobody gives up early, the only money moving is the 5% each person pays on the way out. Whoever leaves first gets back less than they put in. This is a game, not a yield product.
- **Many accounts.** One person can open many accounts. Each costs a full deposit and 0.016 NEAR of storage, so it earns exactly what the same money would earn spread across other people.
- **Patience is time.** A wallet with lost keys looks like a saint, and its deposit stays inside its soul.
- **No DEX pool in v1.** A pool would pay the tax on every trade, so the vault is the exit.
- **The walk is self-reported.** The chain measures how long you waited. The sketch of how you behaved comes from the game and a bot can write its own. It shapes the picture and barely moves the odds of a rare one (Legendary 5.1% to 6.1% across extreme walks).
- **Two clocks.** The tax counts a soul's age from the moment its walker stepped on the bridge. The exit book counts days from the moment the soul crossed. Someone who waits on the bridge for days before crossing pays the lower tax but is booked as leaving early.
- **Coins in transit.** Coins sent with `ft_transfer_call` reach the receiver's balance at once, but they move its clock and pay their tax only when the receiver answers, and only for what it keeps.
- **Dust.** A balance whose share of the vault rounds down to zero cannot `leave`. It can still be sent.
- **Storage.** Each entry keeps 0.016 NEAR for its records; the worst case measured is 1,568 bytes, 0.0157 NEAR. The contract account must also hold the storage of its own code, about 3.4 NEAR, plus a buffer. Whoever deploys it funds that.
- **Pictures.** The picture of a soul comes from the chain's seed and the sketch of the walk. The game draws it in the browser. Wallets get the same drawing from `media/server.mjs`, a small service with no dependencies; `media_base` points at it.

## Try it without a wallet

Run `python -m http.server 7890 --directory web`. On localhost the page offers a simulated chain.

1. Open `http://localhost:7890/?mock=1&speed=60`. Press "Connect a NEAR wallet", then "Put in 1 NEAR and step on". See where the money goes, press "Step on" and see the walk run at sixty times the speed.
2. Answer the three questions at the far side. See your soul and "Deposit inside: 0.984 NEAR".
3. In the same tab open `http://localhost:7890/?mock=1&ahead=864000` to be ten days later. Under "Your souls" see "Earned: 10" and "Breaking today returns: 0.603 NEAR".
4. Press "Collect". See the SABIR under your balance.
5. Press "Break it". See what you would get back and the reason question. Pick one, then see "Who gave up, and why" count it.

## Live on testnet

Play it at https://sabir.heylogram.com (the front page) or go straight to the app at https://dapp.sabir.heylogram.com. A NEAR testnet wallet makes the crossing real; without one the walk is a practice run and nothing is written to the chain.

Two copies of the same contract, differing only in launch parameters:

| Copy | Account | Clock |
| --- | --- | --- |
| Real time | [`sabir-bridge.testnet`](https://testnet.nearblocks.io/address/sabir-bridge.testnet) | 1 SABIR a day, 40-day tax, 1,001-day bridge |
| Fast | [`sabir-bridge-fast.testnet`](https://testnet.nearblocks.io/address/sabir-bridge-fast.testnet) | 1 SABIR a minute, 40-minute tax, 60-day bridge |

To see the whole loop in under an hour, use the fast copy: https://dapp.sabir.heylogram.com/?fast=1. Read it without a wallet:

```sh
near contract call-function as-read-only sabir-bridge-fast.testnet get_state json-args {} network-config testnet now
near contract call-function as-read-only sabir-bridge-fast.testnet get_exit_book json-args {} network-config testnet now
```

`scripts/testnet-flow.sh` runs the story with two testnet accounts. Launch parameters of the real-time copy:

```json
{"params":{"gate_secs":300,"toll_start":"1000000000000000000000000","entry_fee":"1000000000000000000000000","daily_sabir":"1000000000000000000","bridge_secs":86486400,"max_tax_bps":5000,"min_tax_bps":500,"full_patience_secs":3456000,"seed":"40000000000000000000","sale_vault_bps":200,"sale_creator_bps":100,"creator":"sabir-creator.testnet","media_base":"https://api.heylogram.com/sabir/souls/"}}
```

Tests: `cd contract && cargo test --lib`. There are 143 unit tests. One of them runs a random crowd for 9,000 steps with the contract reloaded from storage at every step, counting NEAR, tokens and seconds from the outside.

## Built with

- Contract: `near-sdk` 5.30.1, `near-contract-standards` 5.30.1, `uint` 0.10.
- Page: `@near-wallet-selector` 10.1.4 (core, modal UI, MyNearWallet, Meteor, HERE), bundled into `web/vendor` so that the page signing your transactions loads no third-party code at run time. The page itself has no build step; `web/vendor/src` rebuilds the bundle.
- Simulation: Node.js.

## Background

The walk comes from an earlier prototype by the same author, in which a player pays to skip a five-minute intro. That prototype has no chain code and is not part of this repository. All code here was written during the hackathon window.

## License

MIT
