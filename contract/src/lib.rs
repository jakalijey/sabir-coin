//! SABIR: a memecoin that only rewards patience.
//!
//! The Gate: everyone enters by waiting, or pays a toll that melts as the wait runs out.
//! The Vault: every fee and toll backs the supply in NEAR.
//! The Exit: leaving or sending costs a tax that shrinks with holding time. Taxed tokens burn,
//! so whoever stays owns a larger share of the vault.
//!
//! All parameters are set at launch. There is no owner and no admin method.
use near_contract_standards::fungible_token::core::FungibleTokenCore;
use near_contract_standards::fungible_token::events::{FtBurn, FtMint, FtTransfer};
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::receiver::ext_ft_receiver;
use near_contract_standards::fungible_token::resolver::FungibleTokenResolver;
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::json_types::U128;
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};

uint::construct_uint! {
    pub struct U256(4);
}

const BPS: u128 = 10_000;
const LOG_CAPACITY: u32 = 64;
const STORAGE_COST: u128 = 5_000_000_000_000_000_000_000; // 0.005 NEAR
const GAS_ON_TRANSFER: Gas = Gas::from_tgas(35);
const GAS_RESOLVE: Gas = Gas::from_tgas(5);
const GAS_ON_LEAVE: Gas = Gas::from_tgas(3);

fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    if c == 0 {
        return 0;
    }
    (U256::from(a) * U256::from(b) / U256::from(c)).as_u128()
}

#[derive(BorshStorageKey)]
#[near]
enum Key {
    Holders,
    Tickets,
    Log,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct Params {
    /// Seconds the gate takes to open for free.
    pub gate_secs: u64,
    /// Toll at the moment of entry. It falls linearly to zero over `gate_secs`.
    pub toll_start: U128,
    /// Paid by everyone on entry. It goes to the vault, net of storage.
    pub entry_fee: U128,
    /// Grant for the first entrant. It falls linearly to zero at `max_entrants`.
    pub base_grant: U128,
    pub max_entrants: u32,
    /// Exit tax at zero patience, in basis points.
    pub max_tax_bps: u16,
    /// Exit tax at full patience, in basis points.
    pub min_tax_bps: u16,
    /// Seconds of holding that bring the tax down to `min_tax_bps`.
    pub full_patience_secs: u64,
    /// Locked in the contract forever so that supply never reaches zero.
    pub seed: U128,
}

#[near(serializers = [borsh])]
pub struct Holder {
    balance: u128,
    clock: u64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct Ticket {
    pub entered: u64,
    pub passed: bool,
    pub waited_secs: u64,
    pub paid: U128,
    pub fee: U128,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct LogEntry {
    pub kind: String,
    pub who: AccountId,
    pub tokens: U128,
    pub near: U128,
    pub detail: u64,
    pub at: u64,
}

#[near(serializers = [json])]
pub struct State {
    pub vault: U128,
    pub total_supply: U128,
    pub burned: U128,
    pub entrants: u32,
    pub tolls_paid: U128,
    pub skippers: u32,
    pub waiters: u32,
    pub exits: u32,
    pub now: u64,
    pub current_grant: U128,
}

#[near(serializers = [json])]
pub struct HolderView {
    pub balance: U128,
    pub clock: u64,
    pub held_secs: u64,
    pub rank: String,
    pub tax_bps: u16,
    pub exit_value: U128,
    pub ticket: Option<Ticket>,
    pub toll_now: U128,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Sabir {
    params: Params,
    holders: LookupMap<AccountId, Holder>,
    tickets: LookupMap<AccountId, Ticket>,
    log: Vector<LogEntry>,
    log_total: u64,
    vault: u128,
    total_supply: u128,
    burned: u128,
    entrants: u32,
    tolls_paid: u128,
    skippers: u32,
    waiters: u32,
    exits: u32,
}

#[near]
impl Sabir {
    #[init]
    pub fn new(params: Params) -> Self {
        require!(params.gate_secs > 0, "gate_secs must be positive");
        require!(params.full_patience_secs > 0, "full_patience_secs must be positive");
        require!(params.max_entrants > 0, "max_entrants must be positive");
        require!((params.max_tax_bps as u128) < BPS, "max_tax_bps must be below 100%");
        require!(params.min_tax_bps <= params.max_tax_bps, "min tax above max tax");
        require!(params.entry_fee.0 > STORAGE_COST, "entry_fee must cover storage");
        require!(params.seed.0 > 0, "seed must be positive");
        let me = env::current_account_id();
        let seed = params.seed.0;
        let mut this = Self {
            params,
            holders: LookupMap::new(Key::Holders),
            tickets: LookupMap::new(Key::Tickets),
            log: Vector::new(Key::Log),
            log_total: 0,
            vault: 0,
            total_supply: seed,
            burned: 0,
            entrants: 0,
            tolls_paid: 0,
            skippers: 0,
            waiters: 0,
            exits: 0,
        };
        this.holders.insert(me.clone(), Holder { balance: seed, clock: Self::now() });
        FtMint { owner_id: &me, amount: U128(seed), memo: Some("seed") }.emit();
        this
    }

    // ---------- The Gate ----------

    /// Step through the door and start the wait.
    #[payable]
    pub fn enter_gate(&mut self) {
        let who = env::predecessor_account_id();
        require!(self.tickets.get(&who).is_none(), "already entered");
        require!(self.entrants < self.params.max_entrants, "the gate is closed");
        let fee = self.params.entry_fee.0;
        let deposit = env::attached_deposit().as_yoctonear();
        require!(deposit >= fee, "attach the entry fee");
        let net = fee - STORAGE_COST;
        self.vault += net;
        self.tickets.insert(
            who.clone(),
            Ticket { entered: Self::now(), passed: false, waited_secs: 0, paid: U128(0), fee: U128(net) },
        );
        self.push_log("enter", &who, 0, fee, 0);
        Self::refund(&who, deposit - fee);
    }

    /// Walk through. Free once the wait is over; before that, attach the current toll.
    #[payable]
    pub fn pass_gate(&mut self) -> U128 {
        let who = env::predecessor_account_id();
        let mut ticket = self
            .tickets
            .get(&who)
            .cloned()
            .unwrap_or_else(|| env::panic_str("enter the gate first"));
        require!(!ticket.passed, "already through the gate");
        require!(self.entrants < self.params.max_entrants, "the gate is closed");
        let now = Self::now();
        let waited = now.saturating_sub(ticket.entered);
        let toll = self.toll_after(waited);
        let deposit = env::attached_deposit().as_yoctonear();
        require!(deposit >= toll, "attach the toll or keep waiting");

        self.vault += toll;
        self.tolls_paid += toll;
        let grant = self.grant_for(ticket.fee.0 + toll, waited);
        require!(grant > 0, "nothing left to grant");

        ticket.passed = true;
        ticket.waited_secs = waited;
        ticket.paid = U128(toll);
        self.tickets.insert(who.clone(), ticket.clone());

        let (old_balance, old_clock) = self
            .holders
            .get(&who)
            .map(|h| (h.balance, h.clock))
            .unwrap_or((0, ticket.entered));
        self.holders.insert(
            who.clone(),
            Holder {
                balance: old_balance + grant,
                clock: Self::blend_clock(old_balance, old_clock, grant, ticket.entered),
            },
        );
        self.total_supply += grant;
        self.entrants += 1;

        if toll > 0 {
            self.skippers += 1;
            self.push_log("skip", &who, grant, toll, waited);
        } else {
            self.waiters += 1;
            self.push_log("wait", &who, grant, 0, waited);
        }
        FtMint {
            owner_id: &who,
            amount: U128(grant),
            memo: Some(if toll > 0 { "paid the toll" } else { "waited" }),
        }
        .emit();
        Self::refund(&who, deposit - toll);
        U128(grant)
    }

    // ---------- The Exit ----------

    /// Burn tokens for their share of the vault, minus the tax. The tax stays in the vault.
    pub fn leave(&mut self, amount: U128) -> Promise {
        let who = env::predecessor_account_id();
        require!(who != env::current_account_id(), "the seed never leaves");
        let amount = amount.0;
        require!(amount > 0, "amount must be positive");
        let tax_bps = self.tax_bps_of(&who);
        let payout = self.exit_value(amount, tax_bps);
        require!(payout > 0, "nothing to claim");

        let holder = self.holders.get_mut(&who).unwrap_or_else(|| env::panic_str("not a holder"));
        require!(holder.balance >= amount, "not enough balance");
        holder.balance -= amount;
        self.total_supply -= amount;
        self.burned += amount;
        self.vault -= payout;
        self.exits += 1;
        self.push_log("leave", &who, amount, payout, tax_bps as u64);
        FtBurn { owner_id: &who, amount: U128(amount), memo: Some("left through the vault") }.emit();
        Promise::new(who)
            .transfer(NearToken::from_yoctonear(payout))
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_ON_LEAVE).on_leave(U128(payout)))
    }

    /// A failed payout goes back to the vault.
    #[private]
    pub fn on_leave(&mut self, payout: U128) {
        if env::promise_result_checked(0, 0).is_err() {
            self.vault += payout.0;
        }
    }

    // ---------- Views ----------

    pub fn get_params(&self) -> Params {
        self.params.clone()
    }

    pub fn get_state(&self) -> State {
        State {
            vault: U128(self.vault),
            total_supply: U128(self.total_supply),
            burned: U128(self.burned),
            entrants: self.entrants,
            tolls_paid: U128(self.tolls_paid),
            skippers: self.skippers,
            waiters: self.waiters,
            exits: self.exits,
            now: Self::now(),
            current_grant: U128(self.curve_grant()),
        }
    }

    pub fn get_holder(&self, account_id: AccountId) -> HolderView {
        let (balance, clock) = self.holders.get(&account_id).map(|h| (h.balance, h.clock)).unwrap_or((0, 0));
        let held = if balance > 0 { Self::now().saturating_sub(clock) } else { 0 };
        let tax = self.tax_after(held);
        let ticket = self.tickets.get(&account_id).cloned();
        let toll_now = match &ticket {
            Some(t) if !t.passed => self.toll_after(Self::now().saturating_sub(t.entered)),
            Some(_) => 0,
            None => self.params.toll_start.0,
        };
        HolderView {
            balance: U128(balance),
            clock,
            held_secs: held,
            rank: self.rank_after(held).to_string(),
            tax_bps: tax,
            exit_value: U128(self.exit_value(balance, tax)),
            ticket,
            toll_now: U128(toll_now),
        }
    }

    /// Most recent entries first. The full history is in the NEP-297 events.
    pub fn get_log(&self, limit: u32) -> Vec<LogEntry> {
        let len = self.log.len();
        let total = self.log_total;
        let take = (limit.min(LOG_CAPACITY) as u64).min(total);
        let mut out = Vec::new();
        for i in 0..take {
            let idx = ((total - 1 - i) % LOG_CAPACITY as u64) as u32;
            if idx < len {
                out.push(self.log[idx].clone());
            }
        }
        out
    }
}

impl Sabir {
    fn now() -> u64 {
        env::block_timestamp_ms() / 1000
    }

    fn refund(who: &AccountId, amount: u128) {
        if amount > 0 {
            let _ = Promise::new(who.clone()).transfer(NearToken::from_yoctonear(amount));
        }
    }

    fn toll_after(&self, waited: u64) -> u128 {
        let d = self.params.gate_secs;
        if waited >= d {
            return 0;
        }
        mul_div(self.params.toll_start.0, (d - waited) as u128, d as u128)
    }

    fn tax_after(&self, held: u64) -> u16 {
        let (max, min) = (self.params.max_tax_bps as u128, self.params.min_tax_bps as u128);
        let full = self.params.full_patience_secs;
        if held >= full {
            return min as u16;
        }
        (max - (max - min) * held as u128 / full as u128) as u16
    }

    fn tax_bps_of(&self, who: &AccountId) -> u16 {
        let holder = self.holders.get(who).unwrap_or_else(|| env::panic_str("not a holder"));
        self.tax_after(Self::now().saturating_sub(holder.clock))
    }

    fn rank_after(&self, held: u64) -> &'static str {
        match held as u128 * 100 / self.params.full_patience_secs as u128 {
            0..=9 => "Restless",
            10..=34 => "Waiting",
            35..=69 => "Steady",
            70..=99 => "Stone",
            _ => "Mountain",
        }
    }

    fn exit_value(&self, amount: u128, tax_bps: u16) -> u128 {
        let gross = mul_div(amount, self.vault, self.total_supply);
        gross - mul_div(gross, tax_bps as u128, BPS)
    }

    fn curve_grant(&self) -> u128 {
        let cap = self.params.max_entrants as u128;
        mul_div(self.params.base_grant.0, cap - self.entrants as u128, cap)
    }

    /// The curve grant, cut so that entering and leaving at once can never profit from the vault.
    /// `self.vault` already includes the entrant's `cost`.
    fn grant_for(&self, cost: u128, waited: u64) -> u128 {
        let curve = self.curve_grant();
        let keep = BPS - self.tax_after(waited) as u128;
        let v = U256::from(self.vault) * U256::from(keep);
        let c = U256::from(cost) * U256::from(BPS);
        if v <= c {
            return curve;
        }
        let limit = c * U256::from(self.total_supply) / (v - c);
        if limit < U256::from(curve) {
            limit.as_u128()
        } else {
            curve
        }
    }

    fn blend_clock(old_balance: u128, old_clock: u64, added: u128, added_clock: u64) -> u64 {
        let total = old_balance + added;
        if total == 0 {
            return added_clock;
        }
        let weighted = U256::from(old_balance) * U256::from(old_clock) + U256::from(added) * U256::from(added_clock);
        (weighted / U256::from(total)).as_u64()
    }

    fn push_log(&mut self, kind: &str, who: &AccountId, tokens: u128, near: u128, detail: u64) {
        let entry = LogEntry {
            kind: kind.to_string(),
            who: who.clone(),
            tokens: U128(tokens),
            near: U128(near),
            detail,
            at: Self::now(),
        };
        let idx = (self.log_total % LOG_CAPACITY as u64) as u32;
        if idx < self.log.len() {
            self.log[idx] = entry;
        } else {
            self.log.push(entry);
        }
        self.log_total += 1;
    }

    /// Moves tokens from sender to receiver and burns the tax. Returns the net amount delivered.
    fn taxed_transfer(&mut self, sender: &AccountId, receiver: &AccountId, amount: u128, memo: Option<&str>) -> u128 {
        require!(amount > 0, "amount must be positive");
        require!(sender != receiver, "sending to yourself does not make you patient");
        require!(*sender != env::current_account_id(), "the seed never leaves");
        require!(*receiver != env::current_account_id(), "use leave to exit");
        require!(self.holders.contains_key(receiver), "receiver is not registered");
        let tax_bps = self.tax_bps_of(sender);
        let tax = mul_div(amount, tax_bps as u128, BPS);
        let net = amount - tax;

        let from = self.holders.get_mut(sender).unwrap_or_else(|| env::panic_str("not a holder"));
        require!(from.balance >= amount, "not enough balance");
        from.balance -= amount;

        let now = Self::now();
        let to = self.holders.get_mut(receiver).unwrap_or_else(|| env::panic_str("receiver is not registered"));
        to.clock = Self::blend_clock(to.balance, to.clock, net, now);
        to.balance += net;

        self.total_supply -= tax;
        self.burned += tax;
        FtTransfer {
            old_owner_id: sender,
            new_owner_id: receiver,
            amount: U128(net),
            memo,
        }
        .emit();
        if tax > 0 {
            FtBurn { owner_id: sender, amount: U128(tax), memo: Some("exit tax") }.emit();
            self.push_log("tax", sender, tax, 0, tax_bps as u64);
        }
        net
    }
}

#[near]
impl FungibleTokenCore for Sabir {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        require!(env::attached_deposit() == NearToken::from_yoctonear(1), "attach exactly 1 yoctoNEAR");
        let sender = env::predecessor_account_id();
        self.taxed_transfer(&sender, &receiver_id, amount.0, memo.as_deref());
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        require!(env::attached_deposit() == NearToken::from_yoctonear(1), "attach exactly 1 yoctoNEAR");
        require!(
            env::prepaid_gas().as_gas() > GAS_ON_TRANSFER.as_gas() + GAS_RESOLVE.as_gas() + Gas::from_tgas(10).as_gas(),
            "more gas is required"
        );
        let sender = env::predecessor_account_id();
        let net = self.taxed_transfer(&sender, &receiver_id, amount.0, memo.as_deref());
        ext_ft_receiver::ext(receiver_id.clone())
            .with_static_gas(GAS_ON_TRANSFER)
            .ft_on_transfer(sender.clone(), U128(net), msg)
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_RESOLVE).ft_resolve_transfer(
                sender,
                receiver_id,
                U128(net),
            ))
            .into()
    }

    fn ft_total_supply(&self) -> U128 {
        U128(self.total_supply)
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.holders.get(&account_id).map(|h| h.balance).unwrap_or(0))
    }
}

#[near]
impl FungibleTokenResolver for Sabir {
    #[private]
    fn ft_resolve_transfer(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128) -> U128 {
        let amount = amount.0;
        let unused = match env::promise_result_checked(0, 64) {
            Ok(value) => near_sdk::serde_json::from_slice::<U128>(&value)
                .map(|v| v.0.min(amount))
                .unwrap_or(amount),
            Err(_) => amount,
        };
        if unused == 0 {
            return U128(amount);
        }
        let back = match self.holders.get_mut(&receiver_id) {
            Some(to) => {
                let back = unused.min(to.balance);
                to.balance -= back;
                back
            }
            None => 0,
        };
        if back > 0 {
            match self.holders.get_mut(&sender_id) {
                Some(from) => {
                    from.balance += back;
                    FtTransfer {
                        old_owner_id: &receiver_id,
                        new_owner_id: &sender_id,
                        amount: U128(back),
                        memo: Some("refund"),
                    }
                    .emit();
                }
                None => {
                    self.total_supply -= back;
                    self.burned += back;
                    FtBurn { owner_id: &receiver_id, amount: U128(back), memo: Some("sender is gone") }.emit();
                }
            }
        }
        U128(amount - back)
    }
}

#[near]
impl FungibleTokenMetadataProvider for Sabir {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Sabir".to_string(),
            symbol: "SABIR".to_string(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 18,
        }
    }
}

#[near]
impl StorageManagement for Sabir {
    #[payable]
    fn storage_deposit(&mut self, account_id: Option<AccountId>, registration_only: Option<bool>) -> StorageBalance {
        let _ = registration_only;
        let who = account_id.unwrap_or_else(env::predecessor_account_id);
        let deposit = env::attached_deposit().as_yoctonear();
        if self.holders.contains_key(&who) {
            Self::refund(&env::predecessor_account_id(), deposit);
        } else {
            require!(deposit >= STORAGE_COST, "attach at least the storage cost");
            self.holders.insert(who, Holder { balance: 0, clock: Self::now() });
            Self::refund(&env::predecessor_account_id(), deposit - STORAGE_COST);
        }
        StorageBalance { total: NearToken::from_yoctonear(STORAGE_COST), available: NearToken::from_yoctonear(0) }
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        require!(env::attached_deposit() == NearToken::from_yoctonear(1), "attach exactly 1 yoctoNEAR");
        require!(amount.map(|a| a.as_yoctonear() == 0).unwrap_or(true), "nothing to withdraw");
        require!(self.holders.contains_key(&env::predecessor_account_id()), "not registered");
        StorageBalance { total: NearToken::from_yoctonear(STORAGE_COST), available: NearToken::from_yoctonear(0) }
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        let _ = force;
        env::panic_str("accounts stay on the record")
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        StorageBalanceBounds {
            min: NearToken::from_yoctonear(STORAGE_COST),
            max: Some(NearToken::from_yoctonear(STORAGE_COST)),
        }
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.holders.contains_key(&account_id).then(|| StorageBalance {
            total: NearToken::from_yoctonear(STORAGE_COST),
            available: NearToken::from_yoctonear(0),
        })
    }
}

#[cfg(test)]
mod tests;
