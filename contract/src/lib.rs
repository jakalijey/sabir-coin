//! SABIR: a memecoin that only patience can earn.
//!
//! The Gate: a five minute walk reads the walker and draws their soul. It pays no coin.
//! The Soul: an NFT that keeps its owner's deposit and earns SABIR for every day it lives.
//! Nobody earns more for arriving first. See `souls`.
//! The Vault: what the impatient leave behind. Tolls, exit taxes, a cut of soul sales and gifts.
//! The Exit: burn SABIR for a share of the vault. Breaking a soul returns the deposit minus a tax
//! that shrinks with age and never reaches zero, so whoever leaves first pays whoever leaves last.
//!
//! All parameters are set at launch. There is no owner. The creator's only power over the contract
//! is moving the address soul pictures are served from.
use near_contract_standards::fungible_token::core::FungibleTokenCore;
use near_contract_standards::fungible_token::events::{FtBurn, FtMint, FtTransfer};
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::receiver::ext_ft_receiver;
use near_contract_standards::non_fungible_token::{NonFungibleToken, TokenId};
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::json_types::{Base64VecU8, U128};
use near_sdk::store::{IterableMap, LookupMap, Vector};
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};

mod souls;
use souls::{Listing, Soul, MAX_SKETCH};

uint::construct_uint! {
    pub struct U256(4);
}

const BPS: u128 = 10_000;
const DAY: u128 = 86_400;
const LOG_CAPACITY: u32 = 64;
/// Registering a holder who never went through the gate.
const STORAGE_COST: u128 = 5_000_000_000_000_000_000_000; // 0.005 NEAR
/// Kept from every entry fee: the ticket, the holder record, the soul and its one possible listing.
const ENTRY_STORAGE: u128 = 16_000_000_000_000_000_000_000; // 0.016 NEAR
/// Smallest gift the vault takes, so that a wall of dust cannot push real entries off the log.
const MIN_GIFT: u128 = 10_000_000_000_000_000_000_000; // 0.01 NEAR
const MAX_MEDIA_BASE: usize = 200;
/// What `ft_transfer_call` keeps for its own work and for the way back. The receiver gets the rest.
const GAS_KEPT: Gas = Gas::from_tgas(40);
/// The least gas a receiver of `ft_transfer_call` is given.
const GAS_MIN_RECEIVER: Gas = Gas::from_tgas(10);
const GAS_RESOLVE: Gas = Gas::from_tgas(10);
const GAS_ON_PAYOUT: Gas = Gas::from_tgas(5);
const ICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Crect width='64' height='64' rx='14' fill='%231C1712'/%3E%3Cpath d='M20 14h24l-9 18 9 18H20l9-18z' fill='%23D4A24A'/%3E%3C/svg%3E";

fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    if c == 0 {
        return 0;
    }
    (U256::from(a) * U256::from(b) / U256::from(c)).as_u128()
}

#[derive(BorshStorageKey)]
#[near]
enum Key {
    /// Never used: the NFT library's per-owner sets start with this prefix byte.
    #[allow(dead_code)]
    Reserved,
    Holders,
    Tickets,
    Log,
    SoulOwners,
    SoulsPerOwner,
    Souls,
    Listings,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct Params {
    /// Seconds the gate takes to open for free.
    pub gate_secs: u64,
    /// Toll at the moment of entry. It falls linearly to zero over `gate_secs`.
    pub toll_start: U128,
    /// Paid on entry. All of it except the storage stays inside the walker's soul.
    pub entry_fee: U128,
    /// SABIR one living soul earns per day. The same for every soul.
    pub daily_sabir: U128,
    /// Seconds the bridge stays open after launch. When it closes nobody can enter and no soul earns.
    pub bridge_secs: u64,
    /// Tax at zero patience, in basis points.
    pub max_tax_bps: u16,
    /// Tax at full patience, in basis points. Leaving never gets cheaper than this.
    pub min_tax_bps: u16,
    /// Seconds of age that bring the tax down to `min_tax_bps`.
    pub full_patience_secs: u64,
    /// Locked in the contract forever so that supply never reaches zero.
    pub seed: U128,
    /// Cut of every soul sale that goes to the vault, in basis points.
    pub sale_vault_bps: u16,
    /// Cut of every soul sale that goes to `creator`, in basis points.
    pub sale_creator_bps: u16,
    /// Receives `sale_creator_bps` of every sale. Its only other power is `set_media_base`.
    pub creator: AccountId,
    /// Soul pictures are served from `<media_base><token_id>.svg`.
    pub media_base: String,
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
    pub deposit: U128,
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
    /// What the impatient left behind. SABIR is the only claim on it.
    pub vault: U128,
    /// NEAR of walkers who have not crossed yet. It is theirs, not the vault's.
    pub waiting: U128,
    /// NEAR inside living souls. It is their owners', not the vault's.
    pub deposits: U128,
    /// SABIR in wallets, including the locked seed.
    pub total_supply: U128,
    /// SABIR earned by living souls and not collected yet.
    pub uncollected: U128,
    pub burned: U128,
    pub entrants: u32,
    pub tolls_paid: U128,
    pub taxes_paid: U128,
    /// NEAR anyone added with `feed_vault`.
    pub gifts: U128,
    pub skippers: u32,
    pub waiters: u32,
    pub exits: u32,
    pub now: u64,
    pub bridge_closes: u64,
    pub souls_minted: u32,
    pub souls_broken: u32,
    pub souls_listed: u32,
    pub sales: u32,
    pub sale_volume: U128,
}

/// Who gave up, when and why. Written by the chain, readable by anyone.
#[near(serializers = [json, borsh])]
#[derive(Clone, Default)]
pub struct ExitBook {
    /// Souls born on each layer: Chaos, Middle, Threshold, Builder.
    pub born_by_layer: [u32; 4],
    /// Souls broken, by the layer they were born on.
    pub broken_by_layer: [u32; 4],
    /// Souls broken, by the reason given: unsaid, money, price, bored, better offer, no trust.
    pub broken_by_reason: [u32; 6],
    /// Souls broken, by fortieths of full patience lived: under 1, 7, 20, 40, and 40 or more.
    /// With a 40-day patience these are days.
    pub broken_by_days: [u32; 5],
}

#[near(serializers = [json])]
pub struct Crossing {
    pub token_id: TokenId,
    /// NEAR now held inside the soul.
    pub deposit: U128,
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
    launched: u64,
    holders: LookupMap<AccountId, Holder>,
    tickets: LookupMap<AccountId, Ticket>,
    log: Vector<LogEntry>,
    log_total: u64,
    vault: u128,
    waiting: u128,
    deposits: u128,
    total_supply: u128,
    burned: u128,
    /// Soul-seconds earned and not collected, as of `ticked_at`.
    uncollected_secs: u128,
    ticked_at: u64,
    entrants: u32,
    tolls_paid: u128,
    taxes_paid: u128,
    gifts: u128,
    skippers: u32,
    waiters: u32,
    exits: u32,
    tokens: NonFungibleToken,
    souls: LookupMap<TokenId, Soul>,
    listings: IterableMap<TokenId, Listing>,
    souls_minted: u32,
    souls_broken: u32,
    sales: u32,
    sale_volume: u128,
    book: ExitBook,
}

#[near]
impl Sabir {
    #[init]
    pub fn new(params: Params) -> Self {
        require!(params.gate_secs > 0, "gate_secs must be positive");
        require!(params.full_patience_secs > 0, "full_patience_secs must be positive");
        require!(params.bridge_secs > params.gate_secs, "the bridge must outlast the gate");
        require!(params.daily_sabir.0 > 0, "daily_sabir must be positive");
        require!((params.max_tax_bps as u128) < BPS, "max_tax_bps must be below 100%");
        require!(params.min_tax_bps <= params.max_tax_bps, "min tax above max tax");
        require!(params.entry_fee.0 > ENTRY_STORAGE, "entry_fee must cover storage");
        require!(params.seed.0 > 0, "seed must be positive");
        require!(
            params.sale_vault_bps as u128 + params.sale_creator_bps as u128 <= BPS / 4,
            "sale cut is too large"
        );
        require!(params.creator != env::current_account_id(), "creator must be a separate account");
        require!(!params.media_base.is_empty(), "media_base is required");
        require!(params.media_base.len() <= MAX_MEDIA_BASE, "media_base is too long");
        let me = env::current_account_id();
        let seed = params.seed.0;
        let now = Self::now();
        let mut this = Self {
            params,
            launched: now,
            holders: LookupMap::new(Key::Holders),
            tickets: LookupMap::new(Key::Tickets),
            log: Vector::new(Key::Log),
            log_total: 0,
            vault: 0,
            waiting: 0,
            deposits: 0,
            total_supply: seed,
            burned: 0,
            uncollected_secs: 0,
            ticked_at: now,
            entrants: 0,
            tolls_paid: 0,
            taxes_paid: 0,
            gifts: 0,
            skippers: 0,
            waiters: 0,
            exits: 0,
            tokens: NonFungibleToken::new(
                Key::SoulOwners,
                me.clone(),
                None::<Key>,
                Some(Key::SoulsPerOwner),
                None::<Key>,
            ),
            souls: LookupMap::new(Key::Souls),
            listings: IterableMap::new(Key::Listings),
            souls_minted: 0,
            souls_broken: 0,
            sales: 0,
            sale_volume: 0,
            book: ExitBook::default(),
        };
        this.holders.insert(me.clone(), Holder { balance: seed, clock: now });
        FtMint { owner_id: &me, amount: U128(seed), memo: Some("seed") }.emit();
        this
    }

    // ---------- The Gate ----------

    /// Step onto the bridge and start the wait. The fee stays the walker's.
    #[payable]
    pub fn enter_gate(&mut self) {
        let who = env::predecessor_account_id();
        require!(self.tickets.get(&who).is_none(), "already entered");
        require!(Self::now() < self.bridge_closes(), "the bridge is closed");
        let fee = self.params.entry_fee.0;
        let attached = env::attached_deposit().as_yoctonear();
        require!(attached >= fee, "attach the entry fee");
        let deposit = fee - ENTRY_STORAGE;
        self.waiting += deposit;
        self.entrants += 1;
        self.tickets.insert(
            who.clone(),
            Ticket { entered: Self::now(), passed: false, waited_secs: 0, paid: U128(0), deposit: U128(deposit) },
        );
        self.push_log("enter", &who, 0, fee, 0);
        Self::refund(&who, attached - fee);
    }

    /// Walk through. Free once the wait is over; before that, attach the current toll.
    /// The walk is rewarded with a soul and nothing else: from here on, only time earns.
    /// `sketch` is how the walk went, as drawn by the game. It only shapes the picture of the
    /// soul: its seed, its age, the deposit inside and every payment come from the chain.
    #[payable]
    pub fn pass_gate(&mut self, sketch: Option<Base64VecU8>) -> Crossing {
        let who = env::predecessor_account_id();
        let mut ticket = self
            .tickets
            .get(&who)
            .cloned()
            .unwrap_or_else(|| env::panic_str("enter the gate first"));
        require!(!ticket.passed, "already through the gate");
        let sketch = sketch.map(|s| s.0).unwrap_or_default();
        require!(sketch.len() <= MAX_SKETCH, "the sketch is too long");
        let now = Self::now();
        let waited = now.saturating_sub(ticket.entered);
        let toll = self.toll_after(waited);
        let attached = env::attached_deposit().as_yoctonear();
        require!(attached >= toll, "attach the toll or keep waiting");

        self.tick();
        let deposit = ticket.deposit.0;
        self.vault += toll;
        self.tolls_paid += toll;
        self.waiting -= deposit;
        self.deposits += deposit;

        ticket.passed = true;
        ticket.waited_secs = waited;
        ticket.paid = U128(toll);
        self.tickets.insert(who.clone(), ticket.clone());

        if !self.holders.contains_key(&who) {
            self.holders.insert(who.clone(), Holder { balance: 0, clock: ticket.entered });
        }
        if toll > 0 {
            self.skippers += 1;
            self.push_log("skip", &who, 0, toll, waited);
        } else {
            self.waiters += 1;
            self.push_log("wait", &who, 0, 0, waited);
        }
        let token_id = self.mint_soul(&who, ticket.entered, waited, toll, deposit, sketch);
        Self::refund(&who, attached - toll);
        Crossing { token_id, deposit: U128(deposit) }
    }

    // ---------- The Vault ----------

    /// Anyone can add to the vault. It comes back out only through SABIR, to those who waited.
    #[payable]
    pub fn feed_vault(&mut self) {
        let who = env::predecessor_account_id();
        let amount = env::attached_deposit().as_yoctonear();
        require!(amount >= MIN_GIFT, "the smallest gift is 0.01 NEAR");
        self.vault += amount;
        self.gifts += amount;
        self.push_log("feed", &who, 0, amount, 0);
    }

    /// The only thing the creator can change: where soul pictures are served from, so that they
    /// do not die with a domain name. It touches no money and no rule.
    #[payable]
    pub fn set_media_base(&mut self, media_base: String) {
        near_sdk::assert_one_yocto();
        require!(env::predecessor_account_id() == self.params.creator, "only the creator can move the pictures");
        require!(!media_base.is_empty(), "media_base is required");
        require!(media_base.len() <= MAX_MEDIA_BASE, "media_base is too long");
        self.params.media_base = media_base;
    }

    // ---------- The Exit ----------

    /// Burn tokens for their share of the vault, minus the tax. The tax stays in the vault.
    /// It cannot be undone, so it takes a key that can attach money and the wallet's confirmation.
    #[payable]
    pub fn leave(&mut self, amount: U128) -> Promise {
        near_sdk::assert_one_yocto();
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
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_ON_PAYOUT).on_payout(U128(payout)))
    }

    /// A failed payout goes back to the vault.
    #[private]
    pub fn on_payout(&mut self, payout: U128) {
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
            waiting: U128(self.waiting),
            deposits: U128(self.deposits),
            total_supply: U128(self.total_supply),
            uncollected: U128(self.tokens_of(self.live_uncollected_secs())),
            burned: U128(self.burned),
            entrants: self.entrants,
            tolls_paid: U128(self.tolls_paid),
            taxes_paid: U128(self.taxes_paid),
            gifts: U128(self.gifts),
            skippers: self.skippers,
            waiters: self.waiters,
            exits: self.exits,
            now: Self::now(),
            bridge_closes: self.bridge_closes(),
            souls_minted: self.souls_minted,
            souls_broken: self.souls_broken,
            souls_listed: self.listings.len(),
            sales: self.sales,
            sale_volume: U128(self.sale_volume),
        }
    }

    /// The public record of who gave up, when and why.
    pub fn get_exit_book(&self) -> ExitBook {
        self.book.clone()
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

    fn bridge_closes(&self) -> u64 {
        self.launched + self.params.bridge_secs
    }

    /// The last moment that counts towards earning.
    fn earning_now(&self) -> u64 {
        Self::now().min(self.bridge_closes())
    }

    fn alive(&self) -> u128 {
        (self.souls_minted - self.souls_broken) as u128
    }

    fn live_uncollected_secs(&self) -> u128 {
        self.uncollected_secs + self.alive() * self.earning_now().saturating_sub(self.ticked_at) as u128
    }

    /// Brings the running total up to now. Must come before anything that changes the number of
    /// living souls or what they have collected.
    fn tick(&mut self) {
        self.uncollected_secs = self.live_uncollected_secs();
        self.ticked_at = self.ticked_at.max(self.earning_now());
    }

    fn tokens_of(&self, secs: u128) -> u128 {
        mul_div(secs, self.params.daily_sabir.0, DAY)
    }

    /// Every SABIR that exists or has been earned counts, collected or not. Collecting early
    /// can never take a larger share than waiting to collect.
    fn live_supply(&self) -> u128 {
        self.total_supply + self.tokens_of(self.live_uncollected_secs())
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
        let gross = mul_div(amount, self.vault, self.live_supply());
        gross - mul_div(gross, tax_bps as u128, BPS)
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

    /// Takes `amount` from the sender and credits what is left after the tax. Returns what was
    /// credited, the tax and its rate. The tax is not burned here and the receiver's clock does
    /// not move here: `ft_transfer` does both at once, `ft_transfer_call` waits for the answer.
    fn move_coins(&mut self, sender: &AccountId, receiver: &AccountId, amount: u128, memo: Option<&str>) -> (u128, u128, u16) {
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

        let to = self.holders.get_mut(receiver).unwrap_or_else(|| env::panic_str("receiver is not registered"));
        to.balance += net;

        FtTransfer { old_owner_id: sender, new_owner_id: receiver, amount: U128(net), memo }.emit();
        (net, tax, tax_bps)
    }

    /// Coins that just arrived pull their owner's clock towards now, by their weight.
    fn age_in(&mut self, who: &AccountId, arrived: u128) {
        let now = Self::now();
        if let Some(holder) = self.holders.get_mut(who) {
            let arrived = arrived.min(holder.balance);
            holder.clock = Self::blend_clock(holder.balance - arrived, holder.clock, arrived, now);
        }
    }

    fn burn_tax(&mut self, sender: &AccountId, tax: u128, tax_bps: u16) {
        if tax > 0 {
            self.total_supply -= tax;
            self.burned += tax;
            FtBurn { owner_id: sender, amount: U128(tax), memo: Some("exit tax") }.emit();
            self.push_log("tax", sender, tax, 0, tax_bps as u64);
        }
    }
}

#[near]
impl FungibleTokenCore for Sabir {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        require!(env::attached_deposit() == NearToken::from_yoctonear(1), "attach exactly 1 yoctoNEAR");
        let sender = env::predecessor_account_id();
        let (net, tax, tax_bps) = self.move_coins(&sender, &receiver_id, amount.0, memo.as_deref());
        self.age_in(&receiver_id, net);
        self.burn_tax(&sender, tax, tax_bps);
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
        let prepaid = env::prepaid_gas().as_gas();
        require!(prepaid > GAS_KEPT.as_gas() + GAS_MIN_RECEIVER.as_gas(), "more gas is required");
        let sender = env::predecessor_account_id();
        let (net, tax, _) = self.move_coins(&sender, &receiver_id, amount.0, memo.as_deref());
        ext_ft_receiver::ext(receiver_id.clone())
            .with_static_gas(Gas::from_gas(prepaid - GAS_KEPT.as_gas()))
            .ft_on_transfer(sender.clone(), U128(net), msg)
            .then(Self::ext(env::current_account_id()).with_static_gas(GAS_RESOLVE).ft_resolve_transfer(
                sender,
                receiver_id,
                U128(net),
                U128(tax),
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
impl Sabir {
    /// How a `ft_transfer_call` ends. What the receiver did not use goes back to the sender with
    /// its part of the tax. The tax is burned, and the receiver's clock moves, only for what was
    /// really delivered: a transfer that did not happen costs nothing.
    #[private]
    pub fn ft_resolve_transfer(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128, tax: U128) -> U128 {
        let (net, tax) = (amount.0, tax.0);
        let unused = match env::promise_result_checked(0, 64) {
            Ok(value) => near_sdk::serde_json::from_slice::<U128>(&value).map(|v| v.0.min(net)).unwrap_or(net),
            Err(_) => net,
        };
        let back = match self.holders.get_mut(&receiver_id) {
            Some(to) => {
                let back = unused.min(to.balance);
                to.balance -= back;
                back
            }
            None => 0,
        };
        let delivered = net - back;
        let tax_burned = mul_div(tax, delivered, net);
        let tax_back = tax - tax_burned;
        let returned = back + tax_back;
        self.age_in(&receiver_id, delivered);
        if returned > 0 {
            match self.holders.get_mut(&sender_id) {
                Some(from) => {
                    from.balance += returned;
                    if back > 0 {
                        FtTransfer {
                            old_owner_id: &receiver_id,
                            new_owner_id: &sender_id,
                            amount: U128(back),
                            memo: Some("refund"),
                        }
                        .emit();
                    }
                }
                None => {
                    self.total_supply -= returned;
                    self.burned += returned;
                    FtBurn { owner_id: &receiver_id, amount: U128(returned), memo: Some("sender is gone") }.emit();
                }
            }
        }
        self.burn_tax(&sender_id, tax_burned, mul_div(tax, BPS, net + tax) as u16);
        U128(delivered)
    }
}

#[near]
impl FungibleTokenMetadataProvider for Sabir {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Sabir".to_string(),
            symbol: "SABIR".to_string(),
            icon: Some(ICON.to_string()),
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
#[cfg(test)]
mod tests_coin;
#[cfg(test)]
mod tests_souls;
