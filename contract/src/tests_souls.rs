use super::*;
use crate::souls::{Broken, SoulView};
use crate::tests::{acc, collect, ctx, enter, fresh, join, params, D as DAY, FEE, NEAR, SEED, STAKE, START, TOKEN};
use near_contract_standards::non_fungible_token::core::NonFungibleTokenCore;
use near_contract_standards::non_fungible_token::metadata::NonFungibleTokenMetadataProvider;
use near_contract_standards::non_fungible_token::NonFungibleTokenEnumeration;
use near_sdk::mock::MockAction;
use near_sdk::serde_json;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{test_vm_config, testing_env, PromiseResult, RuntimeFeesConfig};
use std::collections::{HashMap, HashSet};

const ME: &str = "sabir.testnet";
const MARKET: &str = "market.testnet";

fn list(c: &mut Sabir, who: &str, at: u64, id: &str, price: u128) {
    ctx(who, at, 1);
    c.list_soul(id.to_string(), U128(price));
}

fn register(c: &mut Sabir, who: &str, at: u64) {
    if !c.holders.contains_key(&acc(who)) {
        ctx(who, at, STORAGE_COST);
        c.storage_deposit(None, None);
    }
}

/// Pays `price` and, for a buyer with no record yet, the record.
fn buy(c: &mut Sabir, who: &str, at: u64, id: &str, price: u128) -> u128 {
    let record = if c.holders.contains_key(&acc(who)) { 0 } else { STORAGE_COST };
    ctx(who, at, price + record);
    let asked = c.get_soul(id.to_string()).and_then(|v| v.price).unwrap_or(U128(0));
    c.buy_soul(id.to_string(), asked).0
}

/// The only way a soul changes hands.
fn sell(c: &mut Sabir, from: &str, to: &str, at: u64, id: &str, price: u128) {
    list(c, from, at, id, price);
    buy(c, to, at, id, price);
}

fn smash(c: &mut Sabir, who: &str, at: u64, id: &str, reason: Option<u8>) -> Broken {
    ctx(who, at, 1);
    c.break_soul(id.to_string(), reason)
}

fn owner(c: &Sabir, id: &str) -> String {
    c.nft_token(id.to_string()).unwrap().owner_id.to_string()
}

fn soul(c: &Sabir, id: &str) -> SoulView {
    c.get_soul(id.to_string()).unwrap()
}

fn cut(amount: u128, bps: u128) -> u128 {
    (U256::from(amount) * U256::from(bps) / U256::from(10_000u128)).as_u128()
}

/// NEAR leaving the contract in the call that just ran.
fn transfers() -> Vec<(String, u128)> {
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

fn with_result(who: &str, secs: u64, result: PromiseResult) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc(ME))
        .predecessor_account_id(acc(who))
        .signer_account_id(acc(who))
        .block_timestamp(secs * 1_000_000_000)
        .account_balance(NearToken::from_near(10_000));
    testing_env!(b.build(), test_vm_config(), RuntimeFeesConfig::test(), Default::default(), vec![result]);
}

fn flush(c: &mut Sabir) {
    c.holders.flush();
    c.tickets.flush();
    c.log.flush();
    c.souls.flush();
    c.listings.flush();
    env::state_write(c);
}

// ---------- Minting ----------

#[test]
fn every_crossing_mints_one_soul() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 300, 0);
    let out = c.pass_gate(None);
    assert_eq!(out.token_id, "1");
    assert_eq!(out.deposit.0, STAKE);
    assert!(get_logs().iter().any(|l| l.contains("\"nft_mint\"")));
    assert_eq!(c.nft_total_supply().0, 1);
    assert_eq!(owner(&c, "1"), "a.testnet");
    assert_eq!(c.nft_supply_for_owner(acc("a.testnet")).0, 1);
    let tokens = c.nft_tokens_for_owner(acc("a.testnet"), None, None);
    assert_eq!(tokens.len(), 1);
    let m = tokens[0].metadata.clone().unwrap();
    assert_eq!(m.title.unwrap(), "Soul #1, Builder");
    assert_eq!(m.media.unwrap(), "https://souls.example/1.svg");
    assert_eq!(m.copies, Some(1));
    assert_eq!(m.issued_at.unwrap(), ((START + 300) * 1000).to_string());
    let extra: serde_json::Value = serde_json::from_str(&m.extra.unwrap()).unwrap();
    assert_eq!(extra["layer"], "Builder");
    assert_eq!(extra["waited_secs"], 300);
    assert_eq!(extra["crossed_by"], "a.testnet");
    assert_eq!(extra["deposit"], STAKE.to_string());
    assert_eq!(extra["earned"], "0");
}

#[test]
fn the_deposit_lives_inside_the_soul() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let v = soul(&c, &id);
    assert_eq!(v.deposit.0, STAKE);
    assert_eq!(v.tax_bps, 5000);
    assert_eq!(v.break_value.0, STAKE - cut(STAKE, 5000));
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 0);
    assert_eq!(c.ft_balance_of(acc(ME)).0, SEED);
}

#[test]
fn the_layer_comes_from_the_seconds_waited() {
    let mut c = fresh();
    let cases = [
        (0u64, "Chaos"),
        (59, "Chaos"),
        (60, "Middle"),
        (179, "Middle"),
        (180, "Threshold"),
        (299, "Threshold"),
        (300, "Builder"),
        (900, "Builder"),
    ];
    for (i, (wait, layer)) in cases.iter().enumerate() {
        let who = format!("w{i}.testnet");
        enter(&mut c, &who, START);
        ctx(&who, START + wait, NEAR);
        let id = c.pass_gate(None).token_id;
        let v = soul(&c, &id);
        assert_eq!(v.layer, *layer, "wait {wait}");
        assert_eq!(v.waited_secs, *wait);
    }
    assert_eq!(c.get_exit_book().born_by_layer, [2, 2, 2, 2]);
}

#[test]
fn the_toll_is_recorded_on_the_soul() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 150, NEAR);
    let id = c.pass_gate(None).token_id;
    assert_eq!(soul(&c, &id).toll.0, NEAR / 2);
    assert_eq!(transfers(), vec![("a.testnet".to_string(), NEAR - NEAR / 2)]);
}

#[test]
fn seeds_differ_between_people_in_the_same_block() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    enter(&mut c, "b.testnet", START);
    ctx("a.testnet", START + 300, 0);
    let a = c.pass_gate(None).token_id;
    ctx("b.testnet", START + 300, 0);
    let b = c.pass_gate(None).token_id;
    assert_ne!(soul(&c, &a).seed, soul(&c, &b).seed);
    assert_eq!(soul(&c, &a).seed.len(), 32);
}

#[test]
fn the_sketch_is_stored_as_given() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 300, 0);
    let bytes = vec![7u8; souls::MAX_SKETCH];
    let id = c.pass_gate(Some(Base64VecU8::from(bytes.clone()))).token_id;
    assert_eq!(soul(&c, &id).sketch.0, bytes);
}

#[test]
#[should_panic(expected = "the sketch is too long")]
fn an_oversized_sketch_is_refused() {
    let mut c = fresh();
    enter(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 300, 0);
    c.pass_gate(Some(Base64VecU8::from(vec![7u8; souls::MAX_SKETCH + 1])));
}

#[test]
fn no_seat_limit_stands_in_the_way() {
    let mut c = fresh();
    for i in 0..300 {
        join(&mut c, &format!("p{i}.testnet"), START);
    }
    assert_eq!(c.get_state().souls_minted, 300);
}

// ---------- The rank ----------

#[test]
fn a_soul_ranks_up_with_its_own_age() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    for (days, rank) in [(0u64, "Restless"), (4, "Waiting"), (14, "Steady"), (28, "Stone"), (40, "Mountain")] {
        ctx("a.testnet", START + days * DAY, 0);
        assert_eq!(soul(&c, &id).rank, rank, "day {days}");
    }
}

// ---------- The market ----------

#[test]
fn a_sale_splits_the_price() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    assert_eq!(soul(&c, &id).price.unwrap().0, 2 * NEAR);
    let vault = c.get_state().vault.0;
    let to_seller = buy(&mut c, "b.testnet", START + 500, &id, 2 * NEAR);
    let sent = transfers();
    assert_eq!(to_seller, cut(2 * NEAR, 9700));
    assert_eq!(sent.len(), 2);
    assert!(sent.contains(&("a.testnet".to_string(), cut(2 * NEAR, 9700))));
    assert!(sent.contains(&("creator.testnet".to_string(), cut(2 * NEAR, 100))));
    let s = c.get_state();
    assert_eq!(s.vault.0 - vault, cut(2 * NEAR, 200));
    assert_eq!(s.sales, 1);
    assert_eq!(s.sale_volume.0, 2 * NEAR);
    assert_eq!(owner(&c, &id), "b.testnet");
    assert_eq!(c.nft_supply_for_owner(acc("a.testnet")).0, 0);
    assert_eq!(c.nft_supply_for_owner(acc("b.testnet")).0, 1);
    assert!(c.get_listings(None, None).is_empty());
    assert_eq!(soul(&c, &id).sales, 1);
    assert_eq!(soul(&c, &id).price, None);
    assert_eq!(c.get_log(1)[0].kind, "sold");
}

#[test]
fn not_one_yocto_is_lost_to_rounding() {
    // Every case gets its own thread: the mocked chain keeps its storage per thread.
    for price in [NEAR, NEAR + 1, NEAR + 3, 1_234_567_890_123_456_789_012_345, 7 * NEAR + 13] {
        std::thread::spawn(move || {
            let mut c = fresh();
            let id = join(&mut c, "a.testnet", START);
            list(&mut c, "a.testnet", START + 400, &id, price);
            let vault = c.get_state().vault.0;
            buy(&mut c, "b.testnet", START + 500, &id, price);
            let out: u128 = transfers().iter().map(|t| t.1).sum();
            let kept = c.get_state().vault.0 - vault;
            assert_eq!(out + kept, price);
        })
        .join()
        .unwrap();
    }
}

#[test]
fn what_it_earned_stays_with_the_seller() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 300 + 10 * DAY;
    list(&mut c, "a.testnet", t, &id, NEAR);
    buy(&mut c, "b.testnet", t, &id, NEAR);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 10 * TOKEN);
    ctx("b.testnet", t, 0);
    assert_eq!(soul(&c, &id).earned.0, 0);
    ctx("b.testnet", t + DAY, 0);
    assert_eq!(soul(&c, &id).earned.0, TOKEN);
    assert_eq!(c.get_state().uncollected.0, TOKEN);
}

#[test]
fn the_deposit_travels_with_the_soul() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    buy(&mut c, "b.testnet", START + 500, &id, 2 * NEAR);
    assert_eq!(soul(&c, &id).deposit.0, STAKE);
    assert_eq!(c.get_state().deposits.0, STAKE);
}

#[test]
fn overpaying_comes_back() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    buy(&mut c, "b.testnet", START + 500, &id, 3 * NEAR);
    assert!(transfers().contains(&("b.testnet".to_string(), NEAR)));
}

#[test]
#[should_panic(expected = "attach the full price")]
fn a_new_buyer_also_pays_for_a_record() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    ctx("b.testnet", START + 500, 2 * NEAR);
    c.buy_soul(id, U128(2 * NEAR));
}

#[test]
#[should_panic(expected = "the price has changed")]
fn a_raised_price_is_refused() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, NEAR);
    list(&mut c, "a.testnet", START + 450, &id, NEAR + NEAR / 100);
    ctx("b.testnet", START + 500, 2 * NEAR);
    c.buy_soul(id, U128(NEAR));
}

#[test]
#[should_panic(expected = "the price has changed")]
fn a_lowered_price_is_refused_too() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    list(&mut c, "a.testnet", START + 450, &id, NEAR);
    ctx("b.testnet", START + 500, 2 * NEAR);
    c.buy_soul(id, U128(2 * NEAR));
}

#[test]
fn a_registered_buyer_pays_only_the_price() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    register(&mut c, "b.testnet", START + 450);
    ctx("b.testnet", START + 500, 2 * NEAR);
    c.buy_soul(id.clone(), U128(2 * NEAR));
    assert_eq!(owner(&c, &id), "b.testnet");
    assert!(c.holders.contains_key(&acc("b.testnet")));
}

#[test]
fn relisting_changes_the_price() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    list(&mut c, "a.testnet", START + 401, &id, 3 * NEAR);
    let l = c.get_listings(None, None);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].price.0, 3 * NEAR);
    assert_eq!(l[0].seller, acc("a.testnet"));
}

#[test]
fn delisting_works() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    ctx("a.testnet", START + 500, 1);
    c.delist_soul(id.clone());
    assert!(c.get_listings(None, None).is_empty());
    assert_eq!(soul(&c, &id).price, None);
}

#[test]
fn listing_spam_leaves_the_wall_alone() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let before = c.log_total;
    for i in 0..10 {
        list(&mut c, "a.testnet", START + 400 + i, &id, NEAR);
        ctx("a.testnet", START + 400 + i, 1);
        c.delist_soul(id.clone());
    }
    assert_eq!(c.log_total, before);
}

#[test]
fn listings_come_in_pages() {
    let mut c = fresh();
    for i in 0..60 {
        let who = format!("h{i}.testnet");
        let id = join(&mut c, &who, START);
        list(&mut c, &who, START + 400, &id, NEAR);
    }
    assert_eq!(c.get_listings(None, None).len(), 50);
    assert_eq!(c.get_listings(Some(50), None).len(), 10);
    assert_eq!(c.get_listings(None, Some(5)).len(), 5);
    assert_eq!(c.get_listings(None, Some(1000)).len(), 50);
    assert_eq!(c.nft_tokens(None, Some(1000)).len(), 50);
    assert_eq!(c.get_state().souls_listed, 60);
}

#[test]
#[should_panic(expected = "only the owner can sell")]
fn only_the_owner_can_list() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "b.testnet", START + 400, &id, NEAR);
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn listing_needs_a_signature() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 400, 0);
    c.list_soul(id, U128(NEAR));
}

#[test]
#[should_panic(expected = "price is out of range")]
fn a_price_can_be_too_low() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, NEAR / 100 - 1);
}

#[test]
#[should_panic(expected = "the price is below what breaking this soul returns")]
fn a_soul_cannot_be_listed_below_what_breaking_it_returns() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 300 + 40 * DAY;
    ctx("a.testnet", t, 0);
    let floor = soul(&c, &id).break_value.0;
    list(&mut c, "a.testnet", t, &id, floor - 1);
}

#[test]
fn the_lowest_price_is_what_breaking_returns() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    for days in [0u64, 10, 40, 400] {
        let t = START + 300 + days * DAY;
        ctx("a.testnet", t, 0);
        let floor = soul(&c, &id).break_value.0;
        list(&mut c, "a.testnet", t, &id, floor);
        ctx("a.testnet", t, 0);
        assert_eq!(soul(&c, &id).price.unwrap().0, floor, "day {days}");
    }
}

#[test]
#[should_panic(expected = "price is out of range")]
fn a_price_can_be_too_high() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 1_000_000 * NEAR + 1);
}

#[test]
#[should_panic(expected = "no such soul")]
fn a_missing_soul_cannot_be_listed() {
    let mut c = fresh();
    list(&mut c, "a.testnet", START, "99", NEAR);
}

#[test]
#[should_panic(expected = "attach the full price")]
fn underpaying_is_refused() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    buy(&mut c, "b.testnet", START + 500, &id, 2 * NEAR - 1);
}

#[test]
#[should_panic(expected = "not for sale")]
fn an_unlisted_soul_cannot_be_bought() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    buy(&mut c, "b.testnet", START + 500, &id, 2 * NEAR);
}

#[test]
#[should_panic(expected = "already yours")]
fn you_cannot_buy_your_own_soul() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    buy(&mut c, "a.testnet", START + 500, &id, 2 * NEAR);
}

#[test]
#[should_panic(expected = "only the seller")]
fn only_the_seller_can_delist() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    ctx("b.testnet", START + 500, 1);
    c.delist_soul(id);
}

#[test]
#[should_panic(expected = "not for sale")]
fn a_sold_soul_cannot_be_sold_twice() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, 2 * NEAR);
    buy(&mut c, "b.testnet", START + 500, &id, 2 * NEAR);
    buy(&mut c, "d.testnet", START + 500, &id, 2 * NEAR);
}

// ---------- Moving a soul ----------

#[test]
fn a_sale_halves_the_age() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 20 * DAY;
    list(&mut c, "a.testnet", t, &id, NEAR);
    buy(&mut c, "b.testnet", t, &id, NEAR);
    ctx("b.testnet", t, 0);
    assert_eq!(soul(&c, &id).age_secs, 10 * DAY);
    ctx("b.testnet", t + DAY, 0);
    assert_eq!(soul(&c, &id).age_secs, 11 * DAY);
}

#[test]
#[should_panic(expected = "a soul changes hands only by being sold")]
fn a_soul_cannot_be_given_away() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    register(&mut c, "b.testnet", START + 400);
    ctx("a.testnet", START + 400, 1);
    c.nft_transfer(acc("b.testnet"), id, None, None);
}

#[test]
#[should_panic(expected = "a soul changes hands only by being sold")]
fn a_soul_cannot_be_handed_to_a_contract() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    register(&mut c, MARKET, START + 400);
    ctx("a.testnet", START + 400, 1);
    let _ = c.nft_transfer_call(acc(MARKET), id, None, None, String::new());
}

#[test]
#[should_panic(expected = "a soul changes hands only by being sold")]
fn nobody_else_can_move_a_soul_either() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    register(&mut c, "b.testnet", START + 400);
    ctx("x.testnet", START + 400, 1);
    c.nft_transfer(acc("b.testnet"), id, None, None);
}

fn refused<T>(call: impl FnOnce() -> T) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = call();
    }))
    .is_err()
}

#[test]
fn a_refused_transfer_changes_nothing() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 300 + 10 * DAY;
    list(&mut c, "a.testnet", t, &id, NEAR);
    register(&mut c, "b.testnet", t);
    ctx("a.testnet", t, 1);
    let before = soul(&c, &id);
    let balance = c.ft_balance_of(acc("a.testnet")).0;
    assert!(refused(|| c.nft_transfer(acc("b.testnet"), id.clone(), None, None)));
    ctx("a.testnet", t, 1);
    assert!(refused(|| c.nft_transfer_call(acc("b.testnet"), id.clone(), None, None, String::new())));
    ctx("a.testnet", t, 0);
    let after = soul(&c, &id);
    assert_eq!(after.owner_id, acc("a.testnet"));
    assert_eq!(after.age_secs, before.age_secs);
    assert_eq!(after.earned.0, before.earned.0);
    assert_eq!(after.price.unwrap().0, NEAR);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, balance);
    assert_eq!(c.get_listings(None, None).len(), 1);
}

#[test]
fn every_change_of_owner_pays_its_cut() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let chain = [("a.testnet", "b.testnet", 2 * NEAR), ("b.testnet", "d.testnet", 3 * NEAR), ("d.testnet", "a.testnet", NEAR + NEAR / 7)];
    let mut volume = 0;
    let mut vault = 0;
    for (i, (from, to, price)) in chain.iter().enumerate() {
        let t = START + 400 + i as u64 * DAY;
        list(&mut c, from, t, &id, *price);
        buy(&mut c, to, t, &id, *price);
        assert!(transfers().contains(&("creator.testnet".to_string(), cut(*price, 100))));
        assert_eq!(owner(&c, &id), *to);
        volume += *price;
        vault += cut(*price, 200);
    }
    let s = c.get_state();
    assert_eq!(s.sales, 3);
    assert_eq!(s.sale_volume.0, volume);
    assert_eq!(s.vault.0, vault);
    assert_eq!(soul(&c, &id).sales, 3);
}

// ---------- Breaking a soul ----------

#[test]
fn breaking_returns_the_deposit_minus_the_tax() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 20 * DAY;
    ctx("a.testnet", t, 0);
    let value = soul(&c, &id).break_value.0;
    let out = smash(&mut c, "a.testnet", t, &id, None);
    let tax = cut(STAKE, 2750);
    assert_eq!(out.tax.0, tax);
    assert_eq!(out.refund.0, STAKE - tax);
    assert_eq!(out.refund.0, value);
    assert_eq!(transfers(), vec![("a.testnet".to_string(), STAKE - tax)]);
    assert!(get_logs().iter().any(|l| l.contains("\"nft_burn\"")));
    assert!(get_logs().iter().any(|l| l.contains("\"soul_broken\"")));
    let s = c.get_state();
    assert_eq!(s.vault.0, tax);
    assert_eq!(s.taxes_paid.0, tax);
    assert_eq!(s.deposits.0, 0);
    assert_eq!(s.souls_broken, 1);
    assert!(c.nft_token(id.clone()).is_none());
    assert!(c.get_soul(id).is_none());
    assert_eq!(c.nft_total_supply().0, 0);
    assert_eq!(c.nft_supply_for_owner(acc("a.testnet")).0, 0);
    assert!(c.nft_tokens_for_owner(acc("a.testnet"), None, None).is_empty());
}

#[test]
fn the_tax_on_a_soul_falls_to_a_floor() {
    let mut c = fresh();
    let cases = [(0u64, 5000u128), (10, 3875), (20, 2750), (40, 500), (400, 500)];
    let souls: Vec<_> = (0..cases.len()).map(|i| join(&mut c, &format!("p{i}.testnet"), START)).collect();
    for (i, (day, bps)) in cases.iter().enumerate() {
        let out = smash(&mut c, &format!("p{i}.testnet"), START + (day * DAY).max(300), &souls[i], None);
        assert_eq!(out.tax.0, cut(STAKE, *bps), "day {day}");
    }
}

#[test]
fn breaking_pays_what_the_soul_earned() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 300 + 7 * DAY;
    let out = smash(&mut c, "a.testnet", t, &id, None);
    assert_eq!(out.sabir.0, 7 * TOKEN);
    assert_eq!(c.ft_balance_of(acc("a.testnet")).0, 7 * TOKEN);
    ctx("x.testnet", t + 30 * DAY, 0);
    assert_eq!(c.get_state().uncollected.0, 0, "a broken soul earns nothing more");
}

#[test]
fn the_exit_book_keeps_score() {
    let mut c = fresh();
    enter(&mut c, "skip.testnet", START);
    ctx("skip.testnet", START, NEAR);
    let s = c.pass_gate(None).token_id;
    let a = join(&mut c, "a.testnet", START);
    let b = join(&mut c, "b.testnet", START);
    let d = join(&mut c, "d.testnet", START);
    let e = join(&mut c, "e.testnet", START);
    let born = START + 300;
    smash(&mut c, "skip.testnet", START + 3 * DAY, &s, Some(1));
    assert!(get_logs().iter().any(|l| l.contains("soul_broken") && l.contains("\"reason\":1") && l.contains("\"layer\":\"Chaos\"")));
    smash(&mut c, "a.testnet", born + 10, &a, Some(5));
    smash(&mut c, "b.testnet", born + 10 * DAY, &b, None);
    smash(&mut c, "d.testnet", born + 30 * DAY, &d, Some(200));
    smash(&mut c, "e.testnet", born + 50 * DAY, &e, Some(3));
    let book = c.get_exit_book();
    assert_eq!(book.born_by_layer, [1, 0, 0, 4]);
    assert_eq!(book.broken_by_layer, [1, 0, 0, 4]);
    assert_eq!(book.broken_by_reason, [2, 1, 0, 1, 0, 1]);
    assert_eq!(book.broken_by_days, [1, 1, 1, 1, 1]);
}

#[test]
#[should_panic(expected = "no such soul")]
fn a_soul_breaks_only_once() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    smash(&mut c, "a.testnet", START + 400, &id, None);
    smash(&mut c, "a.testnet", START + 401, &id, None);
}

#[test]
#[should_panic(expected = "only the owner can break")]
fn only_the_owner_can_break() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    smash(&mut c, "b.testnet", START + 400, &id, None);
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn breaking_needs_a_signature() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    ctx("a.testnet", START + 400, 0);
    c.break_soul(id, None);
}

#[test]
#[should_panic(expected = "not for sale")]
fn breaking_cancels_the_listing() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    list(&mut c, "a.testnet", START + 400, &id, NEAR);
    smash(&mut c, "a.testnet", START + 450, &id, None);
    assert!(c.get_listings(None, None).is_empty());
    buy(&mut c, "b.testnet", START + 500, &id, NEAR);
}

#[test]
fn a_second_soul_stays_after_the_first_breaks() {
    let mut c = fresh();
    let a = join(&mut c, "a.testnet", START);
    let b = join(&mut c, "b.testnet", START);
    sell(&mut c, "b.testnet", "a.testnet", START + 400, &b, NEAR);
    assert_eq!(c.nft_supply_for_owner(acc("a.testnet")).0, 2);
    smash(&mut c, "a.testnet", START + 500, &a, None);
    assert_eq!(c.nft_supply_for_owner(acc("a.testnet")).0, 1);
    assert_eq!(c.nft_tokens_for_owner(acc("a.testnet"), None, None)[0].token_id, b);
}

#[test]
fn a_buyer_pays_the_tax_of_half_the_age() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    let t = START + 20 * DAY;
    list(&mut c, "a.testnet", t, &id, NEAR);
    buy(&mut c, "b.testnet", t, &id, NEAR);
    ctx("b.testnet", t, 0);
    let v = soul(&c, &id);
    assert_eq!(v.age_secs, 10 * DAY);
    assert_eq!(v.tax_bps, 3875);
    let out = smash(&mut c, "b.testnet", t, &id, None);
    assert_eq!(out.refund.0, STAKE - cut(STAKE, 3875));
}

#[test]
fn an_old_soul_helps_only_by_its_weight() {
    let mut c = fresh();
    let old = join(&mut c, "a.testnet", START);
    let t = START + 30 * DAY;
    let own = join(&mut c, "b.testnet", t);
    let collected_at = t + 300 + 10 * DAY;
    let bal = collect(&mut c, "b.testnet", collected_at, &own);
    let b_clock = c.holders.get(&acc("b.testnet")).unwrap().clock;
    sell(&mut c, "a.testnet", "b.testnet", collected_at, &old, NEAR);
    let soul_clock = soul(&c, &old).clock;
    let got = collect(&mut c, "b.testnet", collected_at + DAY, &old);
    let expected = Sabir::blend_clock(bal, b_clock, got, soul_clock);
    let h = c.holders.get(&acc("b.testnet")).unwrap();
    assert_eq!(h.clock, expected);
    assert!(h.clock > soul_clock && h.clock < b_clock);
}

#[test]
fn whoever_leaves_first_pays_whoever_leaves_last() {
    let mut c = fresh();
    let people = ["a.testnet", "b.testnet", "d.testnet", "e.testnet"];
    let souls: Vec<_> = people.iter().map(|w| join(&mut c, w, START)).collect();
    let mut got = Vec::new();
    for (i, (who, id)) in people.iter().zip(&souls).enumerate() {
        let t = START + 300 + (50 + 10 * i as u64) * DAY;
        let out = smash(&mut c, who, t, id, None);
        ctx(who, t, 1);
        let v = c.get_holder(acc(who));
        got.push(out.refund.0 + v.exit_value.0);
        let _ = c.leave(v.balance);
    }
    for w in got.windows(2) {
        assert!(w[0] < w[1], "{:?}", got);
    }
    // The last to leave comes out ahead of the deposit; the first does not.
    assert!(got[0] < STAKE);
    assert!(*got.last().unwrap() > STAKE - cut(STAKE, 500));
}

#[test]
fn nobody_gets_more_back_at_once_than_they_put_in() {
    let mut c = fresh();
    let souls: Vec<_> = (0..40).map(|i| (format!("h{i}.testnet"), join(&mut c, &format!("h{i}.testnet"), START))).collect();
    let at = START + 300 + 30 * DAY;
    // Most leave at the worst moment, so the vault is rich per token.
    for (who, id) in &souls[..30] {
        smash(&mut c, who, at, id, None);
        ctx(who, at, 1);
        let bal = c.ft_balance_of(acc(who));
        let _ = c.leave(bal);
    }
    for (name, wait_secs) in [("x.testnet", 300u64), ("y.testnet", 0u64), ("z.testnet", 150u64)] {
        ctx(name, at, FEE);
        c.enter_gate();
        let t = at + wait_secs;
        ctx(name, t, NEAR);
        let toll = c.get_holder(acc(name)).toll_now.0;
        let id = c.pass_gate(None).token_id;
        let out = smash(&mut c, name, t, &id, None);
        ctx(name, t, 0);
        let exit = c.get_holder(acc(name)).exit_value.0;
        let cost = FEE + toll;
        assert!(out.refund.0 + exit <= cost, "{name}: {} > cost {cost}", out.refund.0 + exit);
    }
}

#[test]
fn the_vault_holds_only_impatience() {
    let mut c = fresh();
    for i in 0..30 {
        join(&mut c, &format!("h{i}.testnet"), START);
    }
    ctx("x.testnet", START + 100 * DAY, 0);
    let s = c.get_state();
    assert_eq!(s.vault.0, 0);
    assert_eq!(s.deposits.0, 30 * STAKE);
}

#[test]
fn the_contract_cannot_be_drained_by_breaking() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    smash(&mut c, "a.testnet", START + 400, &id, None);
    assert_eq!(c.ft_balance_of(acc(ME)).0, SEED);
    ctx(ME, START + 500, 1);
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = c.leave(U128(1));
    }));
    assert!(refused.is_err());
}

// ---------- Launch rules and payouts ----------

#[test]
fn a_failed_payout_returns_to_the_vault() {
    let mut c = fresh();
    let before = c.get_state().vault.0;
    with_result(ME, START, PromiseResult::Failed);
    c.on_payout(U128(7));
    assert_eq!(c.get_state().vault.0, before + 7);
    with_result(ME, START, PromiseResult::Successful(Vec::new()));
    c.on_payout(U128(7));
    assert_eq!(c.get_state().vault.0, before + 7);
}

#[test]
fn the_exit_book_follows_the_clock_of_the_copy() {
    ctx("deployer.testnet", START, 0);
    let mut p = params();
    p.full_patience_secs = 2400;
    let mut c = Sabir::new(p);
    let id = join(&mut c, "a.testnet", START);
    smash(&mut c, "a.testnet", START + 300 + 30 * 60, &id, None);
    assert_eq!(c.get_exit_book().broken_by_days, [0, 0, 0, 1, 0]);
}

#[test]
fn the_creator_can_move_the_pictures() {
    let mut c = fresh();
    let id = join(&mut c, "a.testnet", START);
    ctx("creator.testnet", START + 400, 1);
    c.set_media_base("https://new.example/souls/".to_string());
    let m = c.nft_token(id).unwrap().metadata.unwrap();
    assert_eq!(m.media.unwrap(), "https://new.example/souls/1.svg");
    assert_eq!(c.get_params().media_base, "https://new.example/souls/");
}

#[test]
#[should_panic(expected = "only the creator can move the pictures")]
fn nobody_else_can_move_the_pictures() {
    let mut c = fresh();
    ctx("a.testnet", START, 1);
    c.set_media_base("https://evil.example/".to_string());
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn moving_the_pictures_needs_a_signature() {
    let mut c = fresh();
    ctx("creator.testnet", START, 0);
    c.set_media_base("https://new.example/".to_string());
}

#[test]
#[should_panic(expected = "media_base is too long")]
fn a_huge_picture_address_is_refused() {
    let mut c = fresh();
    ctx("creator.testnet", START, 1);
    c.set_media_base("x".repeat(201));
}

#[test]
#[should_panic(expected = "media_base is required")]
fn an_empty_picture_address_is_refused() {
    let mut c = fresh();
    ctx("creator.testnet", START, 1);
    c.set_media_base(String::new());
}

#[test]
fn moving_the_pictures_touches_nothing_else() {
    let mut c = fresh();
    join(&mut c, "a.testnet", START);
    ctx("creator.testnet", START + 400, 1);
    let before = c.get_params();
    c.set_media_base("https://new.example/".to_string());
    let after = c.get_params();
    assert_eq!(before.sale_creator_bps, after.sale_creator_bps);
    assert_eq!(before.sale_vault_bps, after.sale_vault_bps);
    assert_eq!(before.creator, after.creator);
    assert_eq!(before.daily_sabir.0, after.daily_sabir.0);
    assert_eq!(c.get_state().vault.0, 0);
}

fn launch_refused(change: impl FnOnce(&mut Params)) -> bool {
    ctx("deployer.testnet", START, 0);
    let mut p = params();
    change(&mut p);
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Sabir::new(p))).is_err()
}

#[test]
fn launch_refuses_bad_parameters() {
    assert!(launch_refused(|p| {
        p.sale_vault_bps = 2000;
        p.sale_creator_bps = 501;
    }));
    assert!(launch_refused(|p| p.creator = acc(ME)));
    assert!(launch_refused(|p| p.media_base = String::new()));
    assert!(launch_refused(|p| p.media_base = "x".repeat(201)));
    assert!(launch_refused(|p| p.entry_fee = U128(ENTRY_STORAGE)));
    assert!(launch_refused(|p| p.daily_sabir = U128(0)));
    assert!(launch_refused(|p| p.bridge_secs = p.gate_secs));
    assert!(launch_refused(|p| p.seed = U128(0)));
    assert!(launch_refused(|p| p.gate_secs = 0));
    assert!(launch_refused(|p| p.full_patience_secs = 0));
    assert!(launch_refused(|p| p.max_tax_bps = 10_000));
    assert!(launch_refused(|p| p.min_tax_bps = 5001));
    assert!(!launch_refused(|_| {}));
}

#[test]
fn both_standards_describe_themselves() {
    let c = fresh();
    let nft = c.nft_metadata();
    assert_eq!(nft.spec, "nft-1.0.0");
    assert_eq!(nft.symbol, "SOUL");
    assert!(nft.icon.unwrap().starts_with("data:image/svg+xml,"));
    let ft = c.ft_metadata();
    assert_eq!(ft.symbol, "SABIR");
    assert!(ft.icon.unwrap().starts_with("data:image/svg+xml,"));
}

// ---------- Storage ----------

/// Bytes the mocked chain holds right now, counted the way the protocol bills them:
/// key + value + 40 for every record. The per-call counter restarts with each mocked call.
fn stored_bytes() -> u64 {
    let storage = near_sdk::mock::with_mocked_blockchain(|b| b.take_storage());
    let total = storage.iter().map(|(k, v)| (k.len() + v.len()) as u64 + 40).sum();
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc(ME)).account_balance(NearToken::from_near(10_000));
    env::set_blockchain_interface(near_sdk::MockedBlockchain::new(
        b.build(),
        test_vm_config(),
        RuntimeFeesConfig::test(),
        vec![],
        storage,
        Default::default(),
        None,
    ));
    total
}

/// The longest possible account names, the largest sketch and a live listing on every soul.
#[test]
fn the_entry_fee_pays_for_the_worst_case() {
    let mut c = fresh();
    // Fill the log first: it is a ring, so it stops growing after this.
    for i in 0..70 {
        enter(&mut c, &format!("{i:064x}"), START);
    }
    flush(&mut c);
    let base = stored_bytes();
    let byte = env::storage_byte_cost().as_yoctonear();
    // Token numbers at a size the bridge could reach.
    c.souls_minted = 900_000;

    let n: u64 = 40;
    for i in 0..n {
        let who = format!("{:064x}", 1_000 + i);
        enter(&mut c, &who, START);
        ctx(&who, START + 300, 0);
        let id = c.pass_gate(Some(Base64VecU8::from(vec![9u8; souls::MAX_SKETCH]))).token_id;
        list(&mut c, &who, START + 400, &id, NEAR);
    }
    flush(&mut c);
    let listed_bytes = (stored_bytes() - base) / n;
    println!("listed crossing: {listed_bytes} bytes, {} yoctoNEAR, kept {ENTRY_STORAGE}", listed_bytes as u128 * byte);
    assert!(listed_bytes as u128 * byte <= ENTRY_STORAGE, "{listed_bytes} bytes cost more than the entry fee keeps");

    // Every soul moves to a new long-named owner, who paid for their own record.
    let ids: Vec<String> = (0..n).map(|i| (c.souls_minted as u64 - i).to_string()).collect();
    for (k, id) in ids.iter().enumerate() {
        buy(&mut c, &format!("{:064x}", 5_000 + k), START + 500, id, NEAR);
    }
    flush(&mut c);
    let sold_bytes = (stored_bytes() - base) / n;
    println!("after a sale: {sold_bytes} bytes");
    assert!(sold_bytes as u128 * byte <= ENTRY_STORAGE + STORAGE_COST);

    let before = stored_bytes();
    for (k, id) in ids.iter().enumerate() {
        smash(&mut c, &format!("{:064x}", 5_000 + k), START + 600, id, None);
    }
    flush(&mut c);
    let after = stored_bytes();
    println!("after breaking: {} bytes per soul freed", (before - after) / n);
    assert!(after < before);
}

// ---------- Everything at once ----------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A crowd doing random legal things, with the NEAR, the tokens and the seconds counted from the
/// outside, independently of the contract's own arithmetic.
struct World {
    c: Sabir,
    now: u64,
    closes: u64,
    names: Vec<String>,
    held: i128,
    reserved: u128,
    entered: HashMap<String, u64>,
    passed: HashSet<String>,
    alive: u128,
    soul_secs: u128,
}

impl World {
    fn new(crowd: usize, bridge_days: u64) -> Self {
        ctx("deployer.testnet", START, 0);
        let mut p = params();
        p.bridge_secs = bridge_days * DAY;
        let c = Sabir::new(p);
        let mut names: Vec<String> = (0..crowd)
            .map(|i| if i % 5 == 0 { format!("{:064x}", i) } else { format!("p{i}.testnet") })
            .collect();
        names.push(MARKET.to_string());
        let mut w = World {
            c,
            now: START,
            closes: START + bridge_days * DAY,
            names,
            held: 0,
            reserved: 0,
            entered: HashMap::new(),
            passed: HashSet::new(),
            alive: 0,
            soul_secs: 0,
        };
        w.ensure_registered(MARKET);
        w
    }

    /// The chain writes the contract out after every call and reads it back for the next one.
    fn reload(&mut self) {
        flush(&mut self.c);
        let again: Sabir = env::state_read().expect("the contract state was written");
        drop(std::mem::replace(&mut self.c, again));
    }

    fn pick(&self, r: &mut Rng) -> String {
        self.names[r.below(self.names.len() as u64) as usize].clone()
    }

    fn call(&mut self, who: &str, deposit: u128) {
        ctx(who, self.now, deposit);
        self.held += deposit as i128;
    }

    fn drain(&mut self) {
        for (_, amount) in transfers() {
            self.held -= amount as i128;
        }
    }

    fn ensure_registered(&mut self, who: &str) {
        if !self.c.holders.contains_key(&acc(who)) {
            self.call(who, STORAGE_COST);
            self.c.storage_deposit(None, None);
            self.drain();
            self.reserved += STORAGE_COST;
        }
    }

    /// Sometimes an account that was paid no longer exists: the money must come back to the vault.
    fn maybe_fail_a_payout(&mut self, r: &mut Rng) {
        let sent = transfers();
        if sent.is_empty() || r.below(6) != 0 {
            return;
        }
        let (_, amount) = sent[r.below(sent.len() as u64) as usize].clone();
        with_result(ME, self.now, PromiseResult::Failed);
        self.c.on_payout(U128(amount));
        self.held += amount as i128;
    }

    fn live_souls(&mut self) -> Vec<SoulView> {
        ctx("viewer.testnet", self.now, 0);
        let minted = self.c.souls_minted;
        (1..=minted).filter_map(|i| self.c.get_soul(i.to_string())).collect()
    }

    fn all_listings(&mut self) -> Vec<(String, AccountId, u128)> {
        ctx("viewer.testnet", self.now, 0);
        let mut out = Vec::new();
        let mut from = 0u32;
        loop {
            let page = self.c.get_listings(Some(from), Some(50));
            if page.is_empty() {
                break;
            }
            from += page.len() as u32;
            out.extend(page.into_iter().map(|l| (l.token_id, l.seller, l.price.0)));
        }
        out
    }

    fn step(&mut self, r: &mut Rng) {
        match r.below(18) {
            0 | 1 | 2 => {
                let who = self.pick(r);
                if self.entered.contains_key(&who) || self.now >= self.closes {
                    return;
                }
                let extra = r.below(3) as u128 * NEAR / 100;
                self.call(&who, FEE + extra);
                self.c.enter_gate();
                self.drain();
                self.reserved += ENTRY_STORAGE;
                self.entered.insert(who, self.now);
            }
            3 | 4 => {
                let waiting: Vec<String> =
                    self.entered.keys().filter(|w| !self.passed.contains(*w)).cloned().collect();
                if waiting.is_empty() {
                    return;
                }
                let mut sorted = waiting;
                sorted.sort();
                let who = sorted[r.below(sorted.len() as u64) as usize].clone();
                let waited = self.now - self.entered[&who];
                let deposit = if waited >= 300 {
                    0
                } else {
                    ctx(&who, self.now, 0);
                    self.c.get_holder(acc(&who)).toll_now.0 + r.below(2) as u128 * NEAR / 10
                };
                self.call(&who, deposit);
                self.c.pass_gate(if r.below(2) == 0 {
                    None
                } else {
                    Some(Base64VecU8::from(vec![r.below(255) as u8; r.below(257) as usize]))
                });
                self.drain();
                self.alive += 1;
                self.passed.insert(who);
            }
            5 => {
                let who = self.pick(r);
                ctx(&who, self.now, 0);
                let bal = self.c.ft_balance_of(acc(&who)).0;
                if bal == 0 {
                    return;
                }
                let amount = (bal / (1 + r.below(4) as u128)).max(1);
                let tax = self.c.tax_bps_of(&acc(&who));
                if self.c.exit_value(amount, tax) == 0 {
                    return;
                }
                self.call(&who, 1);
                let _ = self.c.leave(U128(amount));
                self.drain();
                // The attached yocto stays in the contract.
                self.reserved += 1;
                self.maybe_fail_a_payout(r);
            }
            6 => {
                let (from, to) = (self.pick(r), self.pick(r));
                ctx(&from, self.now, 0);
                let bal = self.c.ft_balance_of(acc(&from)).0;
                if from == to || bal == 0 {
                    return;
                }
                self.ensure_registered(&to);
                let amount = (bal / (1 + r.below(4) as u128)).max(1);
                self.call(&from, 1);
                self.c.ft_transfer(acc(&to), U128(amount), None);
                self.drain();
                // The attached yocto stays in the contract.
                self.reserved += 1;
            }
            7 => {
                let souls = self.live_souls();
                if souls.is_empty() {
                    return;
                }
                let s = &souls[r.below(souls.len() as u64) as usize];
                let price = s.break_value.0 + r.below(1 << 40) as u128 * 1_000_000_000_000_000;
                let (who, id) = (s.owner_id.to_string(), s.token_id.clone());
                self.call(&who, 1);
                self.c.list_soul(id, U128(price));
                self.drain();
                self.reserved += 1;
            }
            8 => {
                let listed = self.all_listings();
                if listed.is_empty() {
                    return;
                }
                let (id, seller, _) = listed[r.below(listed.len() as u64) as usize].clone();
                self.call(seller.as_str(), 1);
                self.c.delist_soul(id);
                self.drain();
                self.reserved += 1;
            }
            9 | 10 => {
                let listed = self.all_listings();
                if listed.is_empty() {
                    return;
                }
                let (id, seller, price) = listed[r.below(listed.len() as u64) as usize].clone();
                let buyer = self.pick(r);
                if buyer == seller.as_str() {
                    return;
                }
                let record = if self.c.holders.contains_key(&acc(&buyer)) { 0 } else { STORAGE_COST };
                self.call(&buyer, price + record);
                self.c.buy_soul(id, U128(price));
                self.drain();
                self.reserved += record;
                self.maybe_fail_a_payout(r);
            }
            11 => {
                let souls = self.live_souls();
                if souls.is_empty() {
                    return;
                }
                let s = &souls[r.below(souls.len() as u64) as usize];
                let (from, id) = (s.owner_id.to_string(), s.token_id.clone());
                let to = self.pick(r);
                ctx(&from, self.now, 1);
                assert!(refused(|| self.c.nft_transfer(acc(&to), id, None, None)));
            }
            12 => {
                let souls = self.live_souls();
                if souls.is_empty() {
                    return;
                }
                let s = &souls[r.below(souls.len() as u64) as usize];
                let (who, id, value) = (s.owner_id.to_string(), s.token_id.clone(), s.break_value.0);
                let reason = if r.below(3) == 0 { None } else { Some(r.below(9) as u8) };
                self.call(&who, 1);
                let out = self.c.break_soul(id, reason);
                assert_eq!(out.refund.0, value);
                self.drain();
                self.reserved += 1;
                self.alive -= 1;
                self.maybe_fail_a_payout(r);
            }
            13 => {
                let souls = self.live_souls();
                if souls.is_empty() {
                    return;
                }
                let s = &souls[r.below(souls.len() as u64) as usize];
                let (from, id) = (s.owner_id.to_string(), s.token_id.clone());
                ctx(&from, self.now, 1);
                assert!(refused(|| self.c.nft_transfer_call(acc(MARKET), id, None, None, String::new())));
                // The same step sends coins through another contract, which answers at random.
                let (from, to) = (self.pick(r), self.pick(r));
                ctx(&from, self.now, 0);
                let bal = self.c.ft_balance_of(acc(&from)).0;
                if from == to || bal == 0 {
                    return;
                }
                self.ensure_registered(&to);
                let amount = (bal / (1 + r.below(4) as u128)).max(1);
                ctx(&from, self.now, 0);
                let tax = cut(amount, self.c.tax_bps_of(&acc(&from)) as u128);
                let net = amount - tax;
                self.call(&from, 1);
                let _ = self.c.ft_transfer_call(acc(&to), U128(amount), None, String::new());
                self.drain();
                self.reserved += 1;
                let answer = match r.below(4) {
                    0 => PromiseResult::Successful(b"\"0\"".to_vec()),
                    1 => PromiseResult::Successful(format!("\"{}\"", net / 2).into_bytes()),
                    2 => PromiseResult::Successful(format!("\"{net}\"").into_bytes()),
                    _ => PromiseResult::Failed,
                };
                with_result(ME, self.now, answer);
                self.c.ft_resolve_transfer(acc(&from), acc(&to), U128(net), U128(tax));
            }
            14 => {
                let souls = self.live_souls();
                if souls.is_empty() {
                    return;
                }
                let s = &souls[r.below(souls.len() as u64) as usize];
                let (who, id) = (s.owner_id.to_string(), s.token_id.clone());
                self.call(&who, 0);
                self.c.collect(id);
                self.drain();
            }
            15 => {
                let who = self.pick(r);
                let amount = NEAR / 100 + r.below(1 << 30) as u128 * 1_000_000_000_000_000;
                self.call(&who, amount);
                self.c.feed_vault();
                self.drain();
            }
            _ => {
                let from = self.now.min(self.closes);
                self.now += r.below(4 * DAY) + 1;
                let to = self.now.min(self.closes);
                self.soul_secs += self.alive * (to - from) as u128;
            }
        }
    }

    fn check(&mut self, step: usize) {
        let souls = self.live_souls();
        let listed = self.all_listings();
        ctx("viewer.testnet", self.now, 0);
        let s = self.c.get_state();

        let mut sum = self.c.ft_balance_of(acc(ME)).0;
        for n in &self.names {
            sum += self.c.ft_balance_of(acc(n)).0;
        }
        assert_eq!(sum, s.total_supply.0, "step {step}: balances against supply");
        assert_eq!(self.c.ft_balance_of(acc(ME)).0, SEED, "step {step}: the seed");

        // Every second a soul has lived is either still inside it or has been paid out.
        let inside_secs = self.c.live_uncollected_secs();
        let paid_secs = self.soul_secs - inside_secs;
        let paid = s.total_supply.0 + s.burned.0 - SEED;
        let owed = paid_secs * TOKEN / 86_400;
        assert!(paid.abs_diff(owed) <= step as u128 + 2, "step {step}: paid {paid}, owed {owed}");
        let earned: u128 = souls.iter().map(|v| v.earned.0).sum();
        assert!(earned.abs_diff(s.uncollected.0) <= souls.len() as u128 + 1, "step {step}: earned inside the souls");

        let deposits: u128 = souls.iter().map(|v| v.deposit.0).sum();
        assert_eq!(deposits, s.deposits.0, "step {step}: deposits");
        let on_the_bridge = (self.entered.len() - self.passed.len()) as u128;
        assert_eq!(on_the_bridge * STAKE, s.waiting.0, "step {step}: waiting");

        assert_eq!(souls.len() as u128, self.c.nft_total_supply().0, "step {step}: live souls");
        assert_eq!(souls.len() as u128, self.alive, "step {step}: counted from outside");
        assert_eq!(souls.len() as u32, s.souls_minted - s.souls_broken, "step {step}: minted minus broken");
        let mut owned = 0u128;
        for n in &self.names {
            owned += self.c.nft_supply_for_owner(acc(n)).0;
        }
        assert_eq!(owned, souls.len() as u128, "step {step}: owners add up");
        for v in &souls {
            assert_eq!(self.c.nft_token(v.token_id.clone()).unwrap().owner_id, v.owner_id, "step {step}");
            assert!(self.c.holders.contains_key(&v.owner_id), "step {step}: an owner with no record");
            assert!(v.break_value.0 <= v.deposit.0, "step {step}: break value");
        }

        let book = self.c.get_exit_book();
        assert_eq!(book.born_by_layer.iter().sum::<u32>(), s.souls_minted, "step {step}: born");
        assert_eq!(book.broken_by_layer.iter().sum::<u32>(), s.souls_broken, "step {step}: broken by layer");
        assert_eq!(book.broken_by_reason.iter().sum::<u32>(), s.souls_broken, "step {step}: broken by reason");
        assert_eq!(book.broken_by_days.iter().sum::<u32>(), s.souls_broken, "step {step}: broken by days");

        assert_eq!(listed.len() as u32, s.souls_listed, "step {step}: listing count");
        for (id, seller, _) in &listed {
            assert_eq!(*seller, self.c.nft_token(id.clone()).unwrap().owner_id, "step {step}: stale listing {id}");
        }
        for n in &self.names {
            assert!(self.c.get_holder(acc(n)).exit_value.0 <= s.vault.0, "step {step}: a claim above the vault");
        }
        let owed_near = s.vault.0 + s.waiting.0 + s.deposits.0 + self.reserved;
        assert_eq!(self.held, owed_near as i128, "step {step}: NEAR in against NEAR out");
    }
}

#[test]
fn a_random_crowd_never_breaks_the_books() {
    let runs: Vec<_> = [1u64, 2, 3, 0xC0FFEE, 0xDEADBEEF, 987_654_321]
        .into_iter()
        .map(|seed| {
            std::thread::spawn(move || {
                let mut w = World::new(34, 150);
                let mut r = Rng(seed);
                for step in 0..1_500 {
                    w.step(&mut r);
                    w.reload();
                    w.check(step);
                }
                let s = w.c.get_state();
                assert!(s.souls_minted > 0, "seed {seed} never minted");
                assert!(s.sales > 0, "seed {seed} never sold");
                assert!(s.souls_broken > 0, "seed {seed} never broke");
                assert!(s.exits > 0, "seed {seed} never left");
                assert!(w.now > w.closes, "seed {seed} never saw the bridge close");
                format!(
                    "seed {seed}: minted {} sold {} broken {} exits {} vault {}",
                    s.souls_minted, s.sales, s.souls_broken, s.exits, s.vault.0
                )
            })
        })
        .collect();
    for run in runs {
        println!("{}", run.join().unwrap());
    }
}
