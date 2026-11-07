use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

const NEAR: u128 = 1_000_000_000_000_000_000_000_000;
const TOKEN: u128 = 1_000_000_000_000_000_000;
const START: u64 = 1_800_000_000;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn ctx(who: &str, secs: u64, deposit: u128) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc("sabir.testnet"))
        .predecessor_account_id(acc(who))
        .signer_account_id(acc(who))
        .block_timestamp(secs * 1_000_000_000)
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .account_balance(NearToken::from_near(10_000));
    testing_env!(b.build());
}

fn params() -> Params {
    Params {
        gate_secs: 300,
        toll_start: U128(5 * NEAR),
        entry_fee: U128(NEAR / 10),
        base_grant: U128(100_000 * TOKEN),
        max_entrants: 10_000,
        max_tax_bps: 5000,
        min_tax_bps: 500,
        full_patience_secs: 30 * 86_400,
        seed: U128(1_000 * TOKEN),
    }
}

fn fresh() -> Sabir {
    ctx("deployer.testnet", START, 0);
    Sabir::new(params())
}

fn enter(c: &mut Sabir, who: &str, at: u64) {
    ctx(who, at, NEAR / 10);
    c.enter_gate();
}

/// Enters and waits out the gate for free.
fn join(c: &mut Sabir, who: &str, at: u64) -> u128 {
    enter(c, who, at);
    ctx(who, at + 300, 0);
    c.pass_gate().0
}

#[test]
fn toll_melts_to_zero() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).toll_now.0, 5 * NEAR);
    ctx("a.testnet", START + 150, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).toll_now.0, 5 * NEAR / 2);
    ctx("a.testnet", START + 300, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).toll_now.0, 0);
}

#[test]
#[should_panic(expected = "attach the toll or keep waiting")]
fn cannot_skip_without_paying() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 60, 0);
    c.pass_gate();
}

#[test]
fn skipper_pays_vault_and_is_recorded() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 60, 4 * NEAR);
    let before = c.get_state().vault.0;
    c.pass_gate();
    let s = c.get_state();
    assert_eq!(s.vault.0 - before, 4 * NEAR);
    assert_eq!(s.skippers, 1);
    assert_eq!(s.tolls_paid.0, 4 * NEAR);
    let t = c.get_holder(acc("a.testnet")).ticket.unwrap();
    assert_eq!(t.waited_secs, 60);
    assert_eq!(t.paid.0, 4 * NEAR);
    assert_eq!(c.get_log(10)[0].kind, "skip");
}

#[test]
fn waiter_passes_free() {
    let mut c = fresh();
    let g = join(&mut c, "a.testnet", START);
    assert!(g > 0);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, g);
    assert_eq!(c.get_state().waiters, 1);
}

#[test]
#[should_panic(expected = "already through the gate")]
fn cannot_pass_twice() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 1000, 0);
    c.pass_gate();
}

#[test]
fn tax_shrinks_with_holding() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    ctx("a.testnet", START, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).tax_bps, 5000);
    ctx("a.testnet", START + 15 * 86_400, 0);
    assert_eq!(c.get_holder(acc("a.testnet")).tax_bps, 2750);
    ctx("a.testnet", START + 30 * 86_400, 0);
    let v = c.get_holder(acc("a.testnet"));
    assert_eq!(v.tax_bps, 500);
    assert_eq!(v.rank, "Mountain");
}

#[test]
fn leaving_early_makes_stayers_richer() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    join(&mut c, "b.testnet", START);
    let now = START + 301;
    ctx("a.testnet", now, 0);
    let s = c.get_state();
    let before = U256::from(s.vault.0) * U256::from(TOKEN) / U256::from(s.total_supply.0);
    let bal = c.ft_balance_of(acc("b.testnet")).0;
    ctx("b.testnet", now, 0);
    let _ = c.leave(U128(bal));
    let s = c.get_state();
    let after = U256::from(s.vault.0) * U256::from(TOKEN) / U256::from(s.total_supply.0);
    assert!(after > before);
    assert_eq!(s.exits, 1);
    assert_eq!(c.ft_balance_of(acc("b.testnet")).0, 0);
}

#[test]
fn entering_and_leaving_at_once_never_profits() {
    let mut c = fresh();
    for i in 0..40 {
        join(&mut c, &format!("h{i}.testnet"), START);
    }
    // Most holders leave, so the vault is rich per token.
    let at = START + 400;
    for i in 0..30 {
        let who = format!("h{i}.testnet");
        let bal = c.ft_balance_of(acc(&who)).0;
        ctx(&who, at, 0);
        let _ = c.leave(U128(bal));
    }
    for (name, wait_secs) in [("x.testnet", 300u64), ("y.testnet", 0u64), ("z.testnet", 150u64)] {
        ctx(name, at, NEAR / 10);
        c.enter_gate();
        let t = at + wait_secs;
        ctx(name, t, 5 * NEAR);
        let toll = c.get_holder(acc(name)).toll_now.0;
        c.pass_gate();
        let cost = NEAR / 10 + toll;
        let view = c.get_holder(acc(name));
        assert!(view.exit_value.0 <= cost, "{name}: exit {} > cost {cost}", view.exit_value.0);
    }
}

#[test]
fn transfer_burns_tax_and_blends_the_clock() {
    let mut c = fresh();
    let a_bal = join(&mut c, "a.testnet", START);
    let b_bal = join(&mut c, "b.testnet", START);
    let day15 = START + 15 * 86_400;
    let supply = c.get_state().total_supply.0;
    ctx("a.testnet", day15, 1);
    c.ft_transfer(acc("b.testnet"), U128(a_bal), None);
    let tax = a_bal * 2750 / 10_000;
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 0);
    assert_eq!(c.ft_balance_of(acc("b.testnet")).0, b_bal + a_bal - tax);
    assert_eq!(c.get_state().total_supply.0, supply - tax);
    ctx("b.testnet", day15, 0);
    let held = c.get_holder(acc("b.testnet")).held_secs;
    assert!(held > 0 && held < 15 * 86_400);
}

#[test]
fn second_wallet_trick_costs_more() {
    let mut c = fresh();
    let g = join(&mut c, "a.testnet", START);
    ctx("a2.testnet", START, STORAGE_COST);
    c.storage_deposit(None, None);
    let day30 = START + 30 * 86_400;
    ctx("a.testnet", day30, 1);
    c.ft_transfer(acc("a2.testnet"), U128(g), None);
    ctx("a2.testnet", day30, 0);
    let v = c.get_holder(acc("a2.testnet"));
    assert_eq!(v.tax_bps, 5000);
    assert_eq!(v.balance.0, g - g * 500 / 10_000);
}

#[test]
#[should_panic(expected = "sending to yourself")]
fn no_self_transfer() {
    let mut c = fresh();
    let g = join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 400, 1);
    c.ft_transfer(acc("a.testnet"), U128(g), None);
}

#[test]
#[should_panic(expected = "the seed never leaves")]
fn seed_is_locked() {
    let mut c = fresh();
    ctx("sabir.testnet", START, 0);
    let _ = c.leave(U128(1));
}

#[test]
fn grant_falls_and_gate_closes() {
    ctx("deployer.testnet", START, 0);
    let mut p = params();
    p.max_entrants = 2;
    let mut c = Sabir::new(p);
    let g1 = join(&mut c, "a.testnet", START);
    let g2 = join(&mut c, "b.testnet", START);
    assert!(g2 < g1);
    ctx("c.testnet", START, NEAR / 10);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.enter_gate()));
    assert!(res.is_err());
}

#[test]
fn vault_is_never_overdrawn() {
    let mut c = fresh();
    for i in 0..20 {
        join(&mut c, &format!("h{i}.testnet"), START);
    }
    let at = START + 40 * 86_400;
    let mut paid: u128 = 0;
    for i in 0..20 {
        let who = format!("h{i}.testnet");
        ctx(&who, at, 0);
        let v = c.get_holder(acc(&who));
        paid += v.exit_value.0;
        let _ = c.leave(v.balance);
    }
    assert!(paid <= 20 * (NEAR / 10));
    assert_eq!(c.get_state().total_supply.0, 1_000 * TOKEN);
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
