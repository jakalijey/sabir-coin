use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

pub(crate) const NEAR: u128 = 1_000_000_000_000_000_000_000_000;
pub(crate) const TOKEN: u128 = 1_000_000_000_000_000_000;
pub(crate) const START: u64 = 1_800_000_000;
pub(crate) const D: u64 = 86_400;
pub(crate) const FEE: u128 = NEAR;
pub(crate) const STAKE: u128 = NEAR - ENTRY_STORAGE;
pub(crate) const SEED: u128 = 40 * TOKEN;

pub(crate) fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

pub(crate) fn ctx(who: &str, secs: u64, deposit: u128) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc("sabir.testnet"))
        .predecessor_account_id(acc(who))
        .signer_account_id(acc(who))
        .block_timestamp(secs * 1_000_000_000)
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .account_balance(NearToken::from_near(10_000));
    testing_env!(b.build());
}

pub(crate) fn params() -> Params {
    Params {
        gate_secs: 300,
        toll_start: U128(NEAR),
        entry_fee: U128(FEE),
        daily_sabir: U128(TOKEN),
        bridge_secs: 1001 * D,
        max_tax_bps: 5000,
        min_tax_bps: 500,
        full_patience_secs: 40 * D,
        seed: U128(SEED),
        sale_vault_bps: 200,
        sale_creator_bps: 100,
        creator: acc("creator.testnet"),
        media_base: "https://souls.example/".to_string(),
    }
}

pub(crate) fn fresh() -> Sabir {
    ctx("deployer.testnet", START, 0);
    Sabir::new(params())
}

pub(crate) fn enter(c: &mut Sabir, who: &str, at: u64) {
    ctx(who, at, FEE);
    c.enter_gate();
}

/// Enters, waits out the gate for free and returns the soul.
pub(crate) fn join(c: &mut Sabir, who: &str, at: u64) -> TokenId {
    enter(c, who, at);
    ctx(who, at + 300, 0);
    c.pass_gate(None).token_id
}

pub(crate) fn collect(c: &mut Sabir, who: &str, at: u64, id: &str) -> u128 {
    ctx(who, at, 0);
    c.collect(id.to_string()).0
}

fn wide(a: u128, b: u128, c: u128) -> u128 {
    (U256::from(a) * U256::from(b) / U256::from(c)).as_u128()
}

// ---------- The gate ----------

#[test]
fn toll_melts_to_zero() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).toll_now.0, NEAR);
    ctx("a.testnet", START + 150, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).toll_now.0, NEAR / 2);
    ctx("a.testnet", START + 300, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).toll_now.0, 0);
}

#[test]
#[should_panic(expected = "attach the toll or keep waiting")]
fn cannot_skip_without_paying() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 60, 0);
    c.pass_gate(None);
}

#[test]
fn skipper_pays_the_vault_and_is_recorded() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 60, NEAR);
    c.pass_gate(None);
    let toll = NEAR * 240 / 300;
    let s = c.get_state();
    assert_eq!(s.vault.0, toll);
    assert_eq!(s.skippers, 1);
    assert_eq!(s.tolls_paid.0, toll);
    let t = c.get_holder(acc("a.testnet")).ticket.unwrap();
    assert_eq!(t.waited_secs, 60);
    assert_eq!(t.paid.0, toll);
    assert_eq!(c.get_log(10)[0].kind, "skip");
}

#[test]
fn waiter_passes_free() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    let s = c.get_state();
    assert_eq!(s.waiters, 1);
    assert_eq!(s.vault.0, 0);
    assert_eq!(s.tolls_paid.0, 0);
}

#[test]
#[should_panic(expected = "already through the gate")]
fn cannot_pass_twice() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 1000, 0);
    c.pass_gate(None);
}

#[test]
#[should_panic(expected = "already entered")]
fn cannot_enter_twice() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    enter(&mut c, "a.testnet", START + 1);
}

#[test]
#[should_panic(expected = "attach the entry fee")]
fn the_fee_is_required() {
    let mut c = fresh();
    ctx("a.testnet", START, FEE - 1);
    c.enter_gate();
}

#[test]
#[should_panic(expected = "enter the gate first")]
fn nobody_crosses_without_a_ticket() {
    let mut c = fresh();
    ctx("a.testnet", START + 400, 0);
    c.pass_gate(None);
}

#[test]
fn the_entry_fee_stays_the_walkers() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    let s = c.get_state();
    assert_eq!(s.vault.0, 0, "nobody's fee reaches the vault at the door");
    assert_eq!(s.waiting.0, STAKE);
    assert_eq!(s.deposits.0, 0);
    assert_eq!(s.entrants, 1);
    ctx("a.testnet", START + 300, 0);
    let out = c.pass_gate(None);
    let s = c.get_state();
    assert_eq!(out.deposit.0, STAKE);
    assert_eq!(s.vault.0, 0);
    assert_eq!(s.waiting.0, 0);
    assert_eq!(s.deposits.0, STAKE);
}

#[test]
fn a_ticket_never_expires() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 300 * D, 0);
    let out = c.pass_gate(None);
    assert_eq!(out.deposit.0, STAKE);
    assert_eq!(c.get_state().waiting.0, 0);
}

// ---------- Earning ----------

#[test]
fn the_walk_pays_no_coin() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 300, 0);
    let s = c.get_state();
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 0);
    assert_eq!(s.total_supply.0, SEED);
    assert_eq!(s.uncollected.0, 0);
}

#[test]
fn a_soul_earns_one_sabir_a_day() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let born = START + 300;
    for days in [1u64, 2, 40, 365, 1000] {
        ctx("a.testnet", born + days * D, 0);
        assert_eq!(c.get_soul(id.clone()).unwrap().earned.0, days as u128 * TOKEN, "day {days}");
    }
    ctx("a.testnet", born + D / 2, 0);
    assert_eq!(c.get_soul(id).unwrap().earned.0, TOKEN / 2);
}

#[test]
fn arriving_first_buys_nothing() {
    // The same people, the same wait, three ways of arriving. Each world needs its own thread.
    let spacings = [0u64, 60, 3_600, 86_400];
    let worlds: Vec<_> = spacings
        .into_iter()
        .map(|gap| {
            std::thread::spawn(move || {
                let mut c = fresh();
                let mut souls = Vec::new();
                for i in 0..20u64 {
                    let who = format!("p{i}.testnet");
                    souls.push((who.clone(), START + i * gap, join(&mut c, &who, START + i * gap)));
                }
                for (who, at, id) in souls {
                    let got = collect(&mut c, &who, at + 300 + 40 * D, &id);
                    assert_eq!(got, 40 * TOKEN, "{who} with gap {gap}");
                }
            })
        })
        .collect();
    for w in worlds {
        w.join().unwrap();
    }
}

#[test]
fn a_crowd_on_the_bridge_takes_nothing_from_the_first_to_cross() {
    let mut c = fresh();
    for i in 0..500 {
        enter(&mut c, &format!("p{i}.testnet"), START);
    }
    ctx("p0.testnet", START + 300, 0);
    let id = c.pass_gate(None).token_id;
    let got = collect(&mut c, "p0.testnet", START + 300 + D, &id);
    assert_eq!(got, TOKEN);
    assert_eq!(c.get_state().waiting.0, 499 * STAKE);
}

#[test]
fn paying_the_toll_earns_no_more_than_waiting() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START, NEAR);
    let a = c.pass_gate(None).token_id;
    let b = join(&mut c, "b.testnet", START);
    ctx("b.testnet", START + 300 + D, 0);
    let (ae, be) = (c.get_soul(a).unwrap().earned.0, c.get_soul(b).unwrap().earned.0);
    assert_eq!(be, TOKEN);
    // The payer started 300 seconds earlier: that is all the money bought.
    assert_eq!(ae, TOKEN + TOKEN * 300 / 86_400);
}

#[test]
fn collecting_moves_it_to_the_wallet_once() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 300 + 10 * D;
    assert_eq!(collect(&mut c, "a.testnet", t, &id), 10 * TOKEN);
    assert_eq!(collect(&mut c, "a.testnet", t, &id), 0);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 10 * TOKEN);
    assert_eq!(c.get_log(1)[0].kind, "collect");
    let s = c.get_state();
    assert_eq!(s.total_supply.0, SEED + 10 * TOKEN);
    assert_eq!(s.uncollected.0, 0);
    assert_eq!(collect(&mut c, "a.testnet", t + D, &id), TOKEN);
}

#[test]
#[should_panic(expected = "only the owner can collect")]
fn only_the_owner_collects() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    collect(&mut c, "b.testnet", START + D, &id);
}

#[test]
fn the_uncollected_total_is_exact() {
    let mut c = fresh();
    let a = join(&mut c, "a.testnet", START);
    join(&mut c, "b.testnet", START + D);
    let t = START + 300 + 10 * D;
    ctx("x.testnet", t, 0);
    assert_eq!(c.get_state().uncollected.0, 19 * TOKEN);
    collect(&mut c, "a.testnet", t, &a);
    ctx("x.testnet", t, 0);
    assert_eq!(c.get_state().uncollected.0, 9 * TOKEN);
}

// ---------- The bridge closes ----------

fn short_bridge() -> Sabir {
    ctx("deployer.testnet", START, 0);
    let mut p = params();
    p.bridge_secs = 10 * D;
    Sabir::new(p)
}

#[test]
#[should_panic(expected = "the bridge is closed")]
fn nobody_enters_a_closed_bridge() {
    let mut c = short_bridge();
    enter(&mut c, "a.testnet", START + 10 * D);
}

#[test]
fn earning_stops_when_the_bridge_closes() {
    let mut c = short_bridge();
    let id = join(&mut c, "a.testnet", START);
    let later = START + 500 * D;
    ctx("a.testnet", later, 0);
    let expected = TOKEN * (10 * D - 300) as u128 / 86_400;
    assert_eq!(c.get_soul(id.clone()).unwrap().earned.0, expected);
    assert_eq!(c.get_state().uncollected.0, expected);
    assert_eq!(collect(&mut c, "a.testnet", later, &id), expected);
    assert_eq!(collect(&mut c, "a.testnet", later + D, &id), 0);
}

#[test]
fn a_late_walker_can_still_cross_but_earns_nothing() {
    let mut c = short_bridge();
    enter(&mut c, "a.testnet", START + 10 * D - 1);
    ctx("a.testnet", START + 11 * D, 0);
    let id = c.pass_gate(None).token_id;
    ctx("a.testnet", START + 20 * D, 0);
    let v = c.get_soul(id).unwrap();
    assert_eq!(v.earned.0, 0);
    assert_eq!(v.deposit.0, STAKE);
    assert_eq!(c.get_state().uncollected.0, 0);
}

#[test]
fn time_after_the_close_does_not_inflate_the_total() {
    let mut c = short_bridge();
    let a = join(&mut c, "a.testnet", START);
    join(&mut c, "b.testnet", START);
    let t = START + 100 * D;
    collect(&mut c, "a.testnet", t, &a);
    ctx("x.testnet", t + 100 * D, 0);
    let s = c.get_state();
    assert_eq!(s.uncollected.0, TOKEN * (10 * D - 300) as u128 / 86_400);
}

// ---------- The vault and the exit ----------

#[test]
fn tax_shrinks_with_age_and_never_reaches_zero() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    collect(&mut c, "a.testnet", START + D, &id);
    for (day, bps) in [(0u64, 5000u16), (20, 2750), (40, 500), (41, 500), (1000, 500)] {
        ctx("a.testnet", START + day * D, 0);
        assert_eq!(c.get_holder(acc("a.testnet")).tax_bps, bps, "day {day}");
    }
}

#[test]
fn collecting_first_takes_no_larger_share() {
    let mut c = fresh();
    let a = join(&mut c, "a.testnet", START);
    join(&mut c, "b.testnet", START);
    c.vault = 10 * NEAR;
    let t = START + 300 + 10 * D;
    collect(&mut c, "a.testnet", t, &a);
    ctx("a.testnet", t, 0);
    let v = c.get_holder(acc("a.testnet"));
    let age_days = (t - START) as u128;
    let bps = 5000 - 4500 * age_days / (40 * D) as u128;
    let gross = 10 * NEAR / 6; // 10 of the 60 SABIR that exist: seed 40, a 10, b 10
    assert_eq!(v.exit_value.0, gross - wide(gross, bps, 10_000));
}

#[test]
fn leaving_pays_out_and_burns() {
    let mut c = fresh();
    let a = join(&mut c, "a.testnet", START);
    c.vault = 8 * NEAR;
    let t = START + 300 + 40 * D;
    collect(&mut c, "a.testnet", t, &a);
    ctx("a.testnet", t, 1);
    let _ = c.leave(U128(40 * TOKEN));
    let s = c.get_state();
    // Half the SABIR is the seed. Half the vault, minus 5%, goes out.
    assert_eq!(s.vault.0, 8 * NEAR - 4 * NEAR * 95 / 100);
    assert_eq!(s.total_supply.0, SEED);
    assert_eq!(s.burned.0, 40 * TOKEN);
    assert_eq!(s.exits, 1);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 0);
}

#[test]
fn leaving_early_makes_stayers_richer() {
    let mut c = fresh();
    let a = join(&mut c, "a.testnet", START);
    let b = join(&mut c, "b.testnet", START);
    c.vault = 10 * NEAR;
    let t = START + 300 + 5 * D;
    let bal = collect(&mut c, "b.testnet", t, &b);
    ctx("a.testnet", t, 0);
    let before = c.get_soul(a.clone()).unwrap().earned_value.0;
    ctx("b.testnet", t, 1);
    let _ = c.leave(U128(bal));
    ctx("a.testnet", t, 0);
    assert!(c.get_soul(a).unwrap().earned_value.0 > before);
}

#[test]
fn a_newcomer_cannot_reach_into_the_vault() {
    let mut c = fresh();
    c.vault = 1000 * NEAR;
    let id = join(&mut c, "x.testnet", START);
    let t = START + 600;
    let got = collect(&mut c, "x.testnet", t, &id);
    assert!(got > 0);
    ctx("x.testnet", t, 0);
    let out = c.get_holder(acc("x.testnet")).exit_value.0;
    let refund = c.get_soul(id).unwrap().break_value.0;
    assert!(out + refund < FEE, "got back {} of {FEE}", out + refund);
}

#[test]
fn a_crowd_leaves_the_vault_in_one_piece() {
    let mut c = fresh();
    let souls: Vec<_> = (0..20).map(|i| (format!("h{i}.testnet"), join(&mut c, &format!("h{i}.testnet"), START))).collect();
    c.vault = 20 * NEAR;
    let at = START + 300 + 50 * D;
    let mut paid: u128 = 0;
    for (who, id) in &souls {
        collect(&mut c, who, at, id);
        ctx(who, at, 1);
        let v = c.get_holder(acc(who));
        paid += v.exit_value.0;
        let _ = c.leave(v.balance);
    }
    let s = c.get_state();
    assert!(paid <= 20 * NEAR);
    assert_eq!(s.vault.0, 20 * NEAR - paid);
    assert_eq!(s.total_supply.0, SEED);
}

#[test]
fn anyone_can_feed_the_vault() {
    let mut c = fresh();
    ctx("fan.testnet", START, 2 * NEAR);
    c.feed_vault();
    let s = c.get_state();
    assert_eq!(s.vault.0, 2 * NEAR);
    assert_eq!(s.gifts.0, 2 * NEAR);
    let entry = &c.get_log(1)[0];
    assert_eq!(entry.kind, "feed");
    assert_eq!(entry.near.0, 2 * NEAR);
    assert_eq!(entry.who, acc("fan.testnet"));
}

#[test]
#[should_panic(expected = "the smallest gift is 0.01 NEAR")]
fn dust_cannot_be_fed_to_the_vault() {
    let mut c = fresh();
    ctx("fan.testnet", START, NEAR / 100 - 1);
    c.feed_vault();
}

#[test]
fn a_gift_goes_to_those_who_waited() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 300 + 40 * D;
    let got = collect(&mut c, "a.testnet", t, &id);
    ctx("fan.testnet", t, 8 * NEAR);
    c.feed_vault();
    ctx("a.testnet", t, 0);
    // 40 of the 80 SABIR are the seed: half the gift, minus the 5% floor.
    assert_eq!(got, 40 * TOKEN);
    assert_eq!(c.get_holder(acc("a.testnet")).exit_value.0, 4 * NEAR * 95 / 100);
}

#[test]
fn a_fast_copy_runs_the_same_rules_on_a_shorter_clock() {
    ctx("deployer.testnet", START, 0);
    let mut p = params();
    p.daily_sabir = U128(1440 * TOKEN);
    p.full_patience_secs = 2400;
    let mut c = Sabir::new(p);
    let id = join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 300 + 60, 0);
    assert_eq!(c.get_soul(id.clone()).unwrap().earned.0, TOKEN, "one minute, one SABIR");
    ctx("a.testnet", START + 2400, 0);
    assert_eq!(c.get_soul(id).unwrap().tax_bps, 500, "forty minutes bring the floor");
}

// ---------- The coin ----------

#[test]
fn transfer_burns_tax_and_blends_the_clock() {
    let mut c = fresh();
    let a = join(&mut c, "a.testnet", START);
    let b = join(&mut c, "b.testnet", START);
    let t = START + 20 * D;
    let a_bal = collect(&mut c, "a.testnet", t, &a);
    let b_bal = collect(&mut c, "b.testnet", t, &b);
    let supply = c.get_state().total_supply.0;
    ctx("a.testnet", t, 1);
    c.ft_transfer(acc("b.testnet"), U128(a_bal), None);
    let tax = wide(a_bal, 2750, 10_000);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 0);
    assert_eq!(c.ft_balance_of(acc("b.testnet")).0, b_bal + a_bal - tax);
    assert_eq!(c.get_state().total_supply.0, supply - tax);
    ctx("b.testnet", t, 0);
    let held = c.get_holder(acc("b.testnet")).held_secs;
    assert!(held > 0 && held < 20 * D);
}

#[test]
fn second_wallet_trick_costs_more() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    ctx("a2.testnet", START, STORAGE_COST);
    c.storage_deposit(None, None);
    let t = START + 40 * D;
    let g = collect(&mut c, "a.testnet", t, &id);
    ctx("a.testnet", t, 1);
    c.ft_transfer(acc("a2.testnet"), U128(g), None);
    ctx("a2.testnet", t, 0);
    let v = c.get_holder(acc("a2.testnet"));
    assert_eq!(v.tax_bps, 5000);
    assert_eq!(v.balance.0, g - wide(g, 500, 10_000));
}

#[test]
#[should_panic(expected = "sending to yourself")]
fn no_self_transfer() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let g = collect(&mut c, "a.testnet", START + D, &id);
    ctx("a.testnet", START + D, 1);
    c.ft_transfer(acc("a.testnet"), U128(g), None);
}

#[test]
#[should_panic(expected = "the seed never leaves")]
fn seed_is_locked() {
    let mut c = fresh();
    ctx("sabir.testnet", START, 1);
    let _ = c.leave(U128(1));
}

#[test]
#[should_panic(expected = "receiver is not registered")]
fn coins_go_only_to_registered_accounts() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let g = collect(&mut c, "a.testnet", START + D, &id);
    ctx("a.testnet", START + D, 1);
    c.ft_transfer(acc("nobody.testnet"), U128(g), None);
}

#[test]
fn log_keeps_recent_entries() {
    let mut c = fresh();
    for i in 0..80 {
        enter(&mut c, &format!("n{i}.testnet"), START + i);
    }
    let log = c.get_log(64);
    assert_eq!(log.len(), 64);
    assert_eq!(log[0].who, acc("n79.testnet"));
    assert_eq!(log[63].who, acc("n16.testnet"));
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn leaving_takes_the_wallets_confirmation() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    collect(&mut c, "a.testnet", START + 300 + 2 * D, &id);
    ctx("a.testnet", START + 300 + 2 * D, 0);
    let _ = c.leave(U128(TOKEN));
}
