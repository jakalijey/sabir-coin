//! The coin when it travels through other contracts, and the records behind it.
use super::*;
use crate::tests::{acc, collect, ctx, fresh, join, D, NEAR, START, TOKEN};
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{test_vm_config, testing_env, PromiseResult, RuntimeFeesConfig};

const ME: &str = "sabir.testnet";
const A: &str = "a.testnet";
const M: &str = "market.testnet";

/// A holds 50 SABIR old enough for the floor tax of 5%. M is registered and holds nothing.
fn setup() -> (Sabir, u64) {
    let mut c = fresh();
    let id = join(&mut c, A, START);
    ctx(M, START, STORAGE_COST);
    c.storage_deposit(None, None);
    let t = START + 300 + 50 * D;
    assert_eq!(collect(&mut c, A, t, &id), 50 * TOKEN);
    (c, t)
}

fn tax_of(amount: u128) -> u128 {
    amount * 500 / 10_000
}

/// Returns what the receiver was credited and the tax being held.
fn send_call(c: &mut Sabir, t: u64, amount: u128) -> (u128, u128) {
    ctx(A, t, 1);
    let _ = c.ft_transfer_call(acc(M), U128(amount), None, "go".to_string());
    (amount - tax_of(amount), tax_of(amount))
}

fn resolve(c: &mut Sabir, t: u64, net: u128, tax: u128, result: PromiseResult) -> u128 {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc(ME))
        .predecessor_account_id(acc(ME))
        .signer_account_id(acc(A))
        .block_timestamp(t * 1_000_000_000)
        .account_balance(NearToken::from_near(10_000));
    testing_env!(b.build(), test_vm_config(), RuntimeFeesConfig::test(), Default::default(), vec![result]);
    c.ft_resolve_transfer(acc(A), acc(M), U128(net), U128(tax)).0
}

fn answered(unused: u128) -> PromiseResult {
    PromiseResult::Successful(format!("\"{unused}\"").into_bytes())
}

fn balance(c: &Sabir, who: &str) -> u128 {
    c.ft_balance_of(acc(who)).0
}

fn books_add_up(c: &Sabir) {
    assert_eq!(balance(c, ME) + balance(c, A) + balance(c, M), c.ft_total_supply().0);
}

fn near_sent() -> Vec<(String, u128)> {
    let mut out = Vec::new();
    for r in get_created_receipts() {
        for a in r.actions {
            if let MockAction::Transfer { deposit, .. } = a {
                out.push((r.receiver_id.to_string(), deposit.as_yoctonear()));
            }
        }
    }
    out
}

// ---------- ft_transfer_call and its way back ----------

#[test]
fn a_transfer_call_holds_its_tax_until_the_answer() {
    let (mut c, t) = setup();
    let supply = c.ft_total_supply().0;
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    assert_eq!(balance(&c, M), net);
    assert_eq!(balance(&c, A), 40 * TOKEN);
    assert_eq!(c.ft_total_supply().0, supply, "nothing burns before the answer, so no share moves");
    assert_eq!(balance(&c, ME) + balance(&c, A) + balance(&c, M) + tax, supply);
    let mut calls = Vec::new();
    for r in get_created_receipts() {
        for a in r.actions {
            if let MockAction::FunctionCallWeight { method_name, .. } = a {
                calls.push((r.receiver_id.to_string(), String::from_utf8(method_name).unwrap()));
            }
        }
    }
    assert_eq!(calls, vec![(M.to_string(), "ft_on_transfer".to_string()), (ME.to_string(), "ft_resolve_transfer".to_string())]);
}

fn call_with_gas(c: &mut Sabir, t: u64, tgas: u64) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc(ME))
        .predecessor_account_id(acc(A))
        .signer_account_id(acc(A))
        .block_timestamp(t * 1_000_000_000)
        .attached_deposit(NearToken::from_yoctonear(1))
        .prepaid_gas(Gas::from_tgas(tgas))
        .account_balance(NearToken::from_near(10_000));
    testing_env!(b.build());
    let _ = c.ft_transfer_call(acc(M), U128(TOKEN), None, String::new());
}

#[test]
fn the_receiver_gets_the_gas_the_caller_brought() {
    let (mut c, t) = setup();
    call_with_gas(&mut c, t, 200);
    let mut given = None;
    for r in get_created_receipts() {
        for a in r.actions {
            if let MockAction::FunctionCallWeight { method_name, prepaid_gas, .. } = a {
                if method_name == b"ft_on_transfer" {
                    given = Some(prepaid_gas.as_gas());
                }
            }
        }
    }
    assert_eq!(given, Some(Gas::from_tgas(160).as_gas()));
}

#[test]
#[should_panic(expected = "attach exactly 1 yoctoNEAR")]
fn a_transfer_call_needs_a_signature() {
    let (mut c, t) = setup();
    ctx(A, t, 0);
    let _ = c.ft_transfer_call(acc(M), U128(TOKEN), None, String::new());
}

#[test]
#[should_panic(expected = "more gas is required")]
fn a_transfer_call_needs_gas_for_the_way_back() {
    let (mut c, t) = setup();
    call_with_gas(&mut c, t, 50);
}

#[test]
fn a_used_transfer_pays_its_tax() {
    let (mut c, t) = setup();
    let supply = c.ft_total_supply().0;
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    assert_eq!(resolve(&mut c, t, net, tax, answered(0)), net);
    assert_eq!(balance(&c, M), net);
    assert_eq!(balance(&c, A), 40 * TOKEN);
    assert_eq!(c.ft_total_supply().0, supply - tax);
    assert_eq!(c.burned, tax);
    assert!(get_logs().iter().any(|l| l.contains("\"ft_burn\"") && l.contains("exit tax")));
    books_add_up(&c);
}

#[test]
fn the_unused_part_comes_back_with_its_tax() {
    let (mut c, t) = setup();
    let supply = c.ft_total_supply().0;
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    let unused = net / 4;
    let delivered = net - unused;
    let burned = (U256::from(tax) * U256::from(delivered) / U256::from(net)).as_u128();
    assert_eq!(resolve(&mut c, t, net, tax, answered(unused)), delivered);
    assert_eq!(balance(&c, M), delivered);
    assert_eq!(balance(&c, A), 40 * TOKEN + unused + (tax - burned));
    assert_eq!(c.ft_total_supply().0, supply - burned);
    assert!(get_logs().iter().any(|l| l.contains("\"ft_transfer\"") && l.contains("refund")));
    books_add_up(&c);
}

#[test]
fn a_receiver_cannot_send_back_more_than_it_got() {
    let (mut c, t) = setup();
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    assert_eq!(resolve(&mut c, t, net, tax, answered(net * 2)), 0);
    assert_eq!(balance(&c, M), 0);
    assert_eq!(balance(&c, A), 50 * TOKEN);
    books_add_up(&c);
}

#[test]
fn a_transfer_that_failed_costs_nothing() {
    let (mut c, t) = setup();
    let supply = c.ft_total_supply().0;
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    assert_eq!(resolve(&mut c, t, net, tax, PromiseResult::Failed), 0);
    assert_eq!(balance(&c, M), 0);
    assert_eq!(balance(&c, A), 50 * TOKEN);
    assert_eq!(c.ft_total_supply().0, supply);
    assert_eq!(c.burned, 0);
    books_add_up(&c);
}

#[test]
fn a_nonsense_answer_returns_everything() {
    let (mut c, t) = setup();
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    assert_eq!(resolve(&mut c, t, net, tax, PromiseResult::Successful(b"maybe".to_vec())), 0);
    assert_eq!(balance(&c, A), 50 * TOKEN);
    books_add_up(&c);
}

#[test]
fn what_the_receiver_already_spent_cannot_come_back() {
    let (mut c, t) = setup();
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    ctx(M, t, 1);
    c.ft_transfer(acc(A), U128(net - 1000), None);
    assert_eq!(balance(&c, M), 1000);
    let a_before = balance(&c, A);
    let delivered = net - 1000;
    let burned = (U256::from(tax) * U256::from(delivered) / U256::from(net)).as_u128();
    assert_eq!(resolve(&mut c, t, net, tax, answered(net)), delivered);
    assert_eq!(balance(&c, M), 0);
    assert_eq!(balance(&c, A), a_before + 1000 + (tax - burned));
    books_add_up(&c);
}

/// Both hold 40 SABIR, old enough for the floor tax.
fn two_old_holders() -> (Sabir, u64) {
    let mut c = fresh();
    let a = join(&mut c, A, START);
    let m = join(&mut c, M, START);
    let t = START + 300 + 40 * D;
    collect(&mut c, A, t, &a);
    collect(&mut c, M, t, &m);
    (c, t)
}

#[test]
fn a_receiver_with_coins_of_its_own_gives_back_no_more_than_it_got() {
    let (mut c, t) = two_old_holders();
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    assert_eq!(resolve(&mut c, t, net, tax, answered(net * 3)), 0);
    assert_eq!(balance(&c, M), 40 * TOKEN, "the receiver keeps its own coins");
    assert_eq!(balance(&c, A), 40 * TOKEN);
    books_add_up(&c);
}

#[test]
fn a_bounced_transfer_leaves_the_receivers_clock_alone() {
    let (mut c, t) = two_old_holders();
    let before = c.holders.get(&acc(M)).unwrap().clock;
    let (net, tax) = send_call(&mut c, t, 40 * TOKEN);
    resolve(&mut c, t, net, tax, answered(net));
    let h = c.holders.get(&acc(M)).unwrap();
    assert_eq!(h.clock, before);
    assert_eq!(h.balance, 40 * TOKEN);
}

#[test]
fn only_what_was_delivered_moves_the_receivers_clock() {
    let (mut c, t) = two_old_holders();
    let before = c.holders.get(&acc(M)).unwrap().clock;
    let (net, tax) = send_call(&mut c, t, 40 * TOKEN);
    let delivered = net / 2;
    resolve(&mut c, t, net, tax, answered(net - delivered));
    let h = c.holders.get(&acc(M)).unwrap();
    assert_eq!(h.clock, Sabir::blend_clock(40 * TOKEN, before, delivered, t));
    assert_eq!(h.balance, 40 * TOKEN + delivered);
}

#[test]
fn a_refund_to_a_sender_with_no_record_is_burned() {
    let (mut c, t) = setup();
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    c.holders.remove(&acc(A));
    let (supply, burned) = (c.total_supply, c.burned);
    assert_eq!(resolve(&mut c, t, net, tax, answered(net)), 0);
    assert_eq!(balance(&c, M), 0);
    assert_eq!(c.total_supply, supply - net - tax);
    assert_eq!(c.burned, burned + net + tax);
    assert!(get_logs().iter().any(|l| l.contains("\"ft_burn\"") && l.contains("sender is gone")));
}

#[test]
fn a_refund_from_a_receiver_with_no_record_moves_nothing() {
    let (mut c, t) = setup();
    let (net, tax) = send_call(&mut c, t, 10 * TOKEN);
    c.holders.remove(&acc(M));
    assert_eq!(resolve(&mut c, t, net, tax, answered(net)), net);
    assert_eq!(balance(&c, A), 40 * TOKEN);
}

// ---------- ft_transfer ----------

#[test]
#[should_panic(expected = "use leave to exit")]
fn coins_cannot_be_sent_to_the_contract() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    c.ft_transfer(acc(ME), U128(TOKEN), None);
}

#[test]
#[should_panic(expected = "attach exactly 1 yoctoNEAR")]
fn a_transfer_needs_a_signature() {
    let (mut c, t) = setup();
    ctx(A, t, 0);
    c.ft_transfer(acc(M), U128(TOKEN), None);
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn an_empty_transfer_is_refused() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    c.ft_transfer(acc(M), U128(0), None);
}

#[test]
#[should_panic(expected = "not enough balance")]
fn you_cannot_send_what_you_do_not_have() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    c.ft_transfer(acc(M), U128(51 * TOKEN), None);
}

// ---------- leave ----------

fn with_a_full_vault() -> (Sabir, u64) {
    let (mut c, t) = setup();
    ctx("rich.testnet", t, 10 * NEAR);
    c.feed_vault();
    (c, t)
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn leaving_with_nothing_is_refused() {
    let (mut c, t) = with_a_full_vault();
    ctx(A, t, 1);
    let _ = c.leave(U128(0));
}

#[test]
#[should_panic(expected = "not enough balance")]
fn leaving_with_more_than_you_hold_is_refused() {
    let (mut c, t) = with_a_full_vault();
    ctx(A, t, 1);
    let _ = c.leave(U128(51 * TOKEN));
}

#[test]
#[should_panic(expected = "nothing to claim")]
fn an_empty_vault_pays_nothing() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    let _ = c.leave(U128(TOKEN));
}

#[test]
#[should_panic(expected = "not a holder")]
fn a_stranger_cannot_leave() {
    let (mut c, t) = with_a_full_vault();
    ctx("x.testnet", t, 1);
    let _ = c.leave(U128(TOKEN));
}

#[test]
fn leaving_pays_exactly_the_quoted_amount() {
    let (mut c, t) = with_a_full_vault();
    ctx(A, t, 1);
    let quoted = c.get_holder(acc(A)).exit_value.0;
    let _ = c.leave(U128(50 * TOKEN));
    assert_eq!(near_sent(), vec![(A.to_string(), quoted)]);
    assert_eq!(c.get_state().vault.0, 10 * NEAR - quoted);
    assert_eq!(balance(&c, A), 0);
}

// ---------- The records behind every account ----------

#[test]
fn registering_twice_costs_nothing() {
    let mut c = fresh();
    ctx(A, START, STORAGE_COST);
    c.storage_deposit(None, None);
    ctx(A, START, 2 * STORAGE_COST);
    let b = c.storage_deposit(None, None);
    assert_eq!(near_sent(), vec![(A.to_string(), 2 * STORAGE_COST)]);
    assert_eq!(b.total.as_yoctonear(), STORAGE_COST);
}

#[test]
fn the_extra_on_a_registration_comes_back() {
    let mut c = fresh();
    ctx(A, START, 3 * STORAGE_COST);
    c.storage_deposit(None, None);
    assert_eq!(near_sent(), vec![(A.to_string(), 2 * STORAGE_COST)]);
    assert!(c.holders.contains_key(&acc(A)));
}

#[test]
fn registering_someone_else_refunds_the_caller() {
    let mut c = fresh();
    ctx(A, START, 2 * STORAGE_COST);
    c.storage_deposit(Some(acc(M)), None);
    assert_eq!(near_sent(), vec![(A.to_string(), STORAGE_COST)]);
    assert!(c.holders.contains_key(&acc(M)));
    assert!(!c.holders.contains_key(&acc(A)));
}

#[test]
#[should_panic(expected = "attach at least the storage cost")]
fn a_registration_must_be_paid_for() {
    let mut c = fresh();
    ctx(A, START, STORAGE_COST - 1);
    c.storage_deposit(None, None);
}

#[test]
fn nothing_can_be_withdrawn_but_asking_is_allowed() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    assert_eq!(c.storage_withdraw(None).available.as_yoctonear(), 0);
    ctx(A, t, 1);
    assert_eq!(c.storage_withdraw(Some(NearToken::from_yoctonear(0))).available.as_yoctonear(), 0);
}

#[test]
#[should_panic(expected = "nothing to withdraw")]
fn a_withdrawal_of_any_amount_is_refused() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    c.storage_withdraw(Some(NearToken::from_yoctonear(1)));
}

#[test]
#[should_panic(expected = "not registered")]
fn a_stranger_has_nothing_to_withdraw() {
    let (mut c, t) = setup();
    ctx("x.testnet", t, 1);
    c.storage_withdraw(None);
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn a_withdrawal_needs_a_signature() {
    let (mut c, t) = setup();
    ctx(A, t, 0);
    c.storage_withdraw(None);
}

#[test]
#[should_panic(expected = "accounts stay on the record")]
fn a_record_cannot_be_removed() {
    let (mut c, t) = setup();
    ctx(A, t, 1);
    c.storage_unregister(Some(true));
}

#[test]
fn the_record_price_is_public() {
    let (c, _) = setup();
    let bounds = c.storage_balance_bounds();
    assert_eq!(bounds.min.as_yoctonear(), STORAGE_COST);
    assert_eq!(bounds.max.unwrap().as_yoctonear(), STORAGE_COST);
    assert_eq!(c.storage_balance_of(acc(A)).unwrap().total.as_yoctonear(), STORAGE_COST);
    assert!(c.storage_balance_of(acc("x.testnet")).is_none());
}

#[test]
fn blending_nothing_keeps_the_added_clock() {
    assert_eq!(Sabir::blend_clock(0, 5, 0, 9), 9);
}

// ---------- What the page tells people ----------

fn walk_out(c: &mut Sabir, who: &str, id: &str, t: u64) -> f64 {
    ctx(who, t, 1);
    let out = c.break_soul(id.to_string(), None);
    ctx(who, t, 1);
    let v = c.get_holder(acc(who));
    let _ = c.leave(v.balance);
    ((out.refund.0 + v.exit_value.0) / 1_000_000_000_000_000_000) as f64 / 1e6
}

fn the_page_says(days: [u64; 3], says: [f64; 3]) {
    let mut c = fresh();
    let people = ["p1.testnet", "p2.testnet", "p3.testnet"];
    let souls: Vec<_> = people.iter().map(|w| join(&mut c, w, START)).collect();
    for i in 0..3 {
        let got = walk_out(&mut c, people[i], &souls[i], START + 300 + days[i] * D);
        assert!((got - says[i]).abs() < 0.005, "person {i}: the page says {}, the contract pays {got}", says[i]);
    }
}

#[test]
fn the_page_example_where_two_give_up_is_true() {
    the_page_says([3, 20, 60], [0.54, 0.84, 1.30]);
}

#[test]
fn the_page_example_where_nobody_gives_up_is_true() {
    the_page_says([60, 60, 60], [0.95, 0.97, 0.99]);
}
