//! The Souls: what the gate gives back.
//!
//! Every crossing mints one NEP-171 token, and a soul is more than a picture:
//!
//! - It holds money. The walker's deposit sits inside and comes back when the soul is broken.
//! - It earns. A living soul earns the same SABIR per day as every other, whoever came first.
//! - It has its own patience. The tax on breaking it shrinks with its age, down to a floor.
//! - It can be sold here, and only here. Transfers are closed, so every change of owner goes
//!   through `buy_soul`, and part of every price goes to the vault and part to the creator.
//!   A soul is never listed for less than breaking it returns, so the cut is always paid on at
//!   least what the soul holds.
//! - Selling it costs it. Every sale halves its age. What it earned so far stays with the one
//!   who waited for it.
//! - It can be broken. The token burns, the deposit comes back minus the tax, and the tax goes
//!   to the vault for those who stayed. Why it was broken goes into the public exit book.
use super::*;
use near_contract_standards::non_fungible_token::core::NonFungibleTokenCore;
use near_contract_standards::non_fungible_token::events::{NftBurn, NftMint};
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
};
use near_contract_standards::non_fungible_token::{NonFungibleTokenEnumeration, Token};
use near_sdk::assert_one_yocto;
use near_sdk::serde_json::json;

/// Longest sketch of the walk a crossing can store, in bytes.
pub(crate) const MAX_SKETCH: usize = 256;
const ONE_NEAR: u128 = 1_000_000_000_000_000_000_000_000;
const MIN_PRICE: u128 = ONE_NEAR / 100;
const MAX_PRICE: u128 = 1_000_000 * ONE_NEAR;
const PAGE: u64 = 50;
const LAYERS: [&str; 4] = ["Chaos", "Middle", "Threshold", "Builder"];
const SALE_ONLY: &str = "a soul changes hands only by being sold: use list_soul and buy_soul";

#[near(serializers = [borsh])]
pub struct Soul {
    seed: [u8; 16],
    crossed_by: AccountId,
    born: u64,
    clock: u64,
    /// Earnings up to this moment have been paid out.
    earned_to: u64,
    waited_secs: u64,
    toll: u128,
    deposit: u128,
    sales: u32,
    sketch: Vec<u8>,
}

#[near(serializers = [borsh])]
pub struct Listing {
    seller: AccountId,
    price: u128,
}

#[near(serializers = [json])]
pub struct SoulView {
    pub token_id: TokenId,
    pub owner_id: AccountId,
    pub crossed_by: AccountId,
    pub seed: String,
    pub layer: String,
    pub waited_secs: u64,
    pub toll: U128,
    pub born: u64,
    pub clock: u64,
    pub age_secs: u64,
    pub rank: String,
    /// NEAR held inside the soul.
    pub deposit: U128,
    /// Tax on breaking the soul today.
    pub tax_bps: u16,
    /// NEAR its owner would get back by breaking it today.
    pub break_value: U128,
    /// SABIR earned and not collected yet.
    pub earned: U128,
    /// What that SABIR would pay out of the vault today, at the soul's own age.
    pub earned_value: U128,
    pub sales: u32,
    pub price: Option<U128>,
    pub sketch: Base64VecU8,
}

#[near(serializers = [json])]
pub struct ListingView {
    pub token_id: TokenId,
    pub seller: AccountId,
    pub price: U128,
    pub soul: SoulView,
}

#[near(serializers = [json])]
pub struct Broken {
    /// NEAR sent back to the owner.
    pub refund: U128,
    /// NEAR left in the vault.
    pub tax: U128,
    /// SABIR paid out with it.
    pub sabir: U128,
}

#[near]
impl Sabir {
    /// Put a soul up for sale, or change its price. The price cannot be lower than what breaking
    /// the soul returns at this moment.
    #[payable]
    pub fn list_soul(&mut self, token_id: TokenId, price: U128) {
        assert_one_yocto();
        let who = env::predecessor_account_id();
        require!(self.owner_of(&token_id) == who, "only the owner can sell this soul");
        require!(price.0 >= MIN_PRICE && price.0 <= MAX_PRICE, "price is out of range");
        let soul = self.souls.get(&token_id).unwrap_or_else(|| env::panic_str("no such soul"));
        require!(price.0 >= self.break_value(soul), "the price is below what breaking this soul returns");
        self.listings.insert(token_id, Listing { seller: who, price: price.0 });
    }

    /// Take a soul off the market.
    #[payable]
    pub fn delist_soul(&mut self, token_id: TokenId) {
        assert_one_yocto();
        let who = env::predecessor_account_id();
        let listing = self.listings.get(&token_id).unwrap_or_else(|| env::panic_str("this soul is not for sale"));
        require!(listing.seller == who, "only the seller can take it down");
        self.listings.remove(&token_id);
    }

    /// Buy a listed soul at the price the buyer saw. If the seller changed it, nothing happens.
    /// Anything attached above the price comes back. A buyer with no record also pays for one.
    /// Returns what the seller receives.
    #[payable]
    pub fn buy_soul(&mut self, token_id: TokenId, price: U128) -> U128 {
        let buyer = env::predecessor_account_id();
        let listing = self.listings.remove(&token_id).unwrap_or_else(|| env::panic_str("this soul is not for sale"));
        require!(listing.price == price.0, "the price has changed");
        require!(buyer != listing.seller, "this soul is already yours");
        let attached = env::attached_deposit().as_yoctonear();
        let mut cost = listing.price;
        if !self.holders.contains_key(&buyer) {
            cost += STORAGE_COST;
            self.holders.insert(buyer.clone(), Holder { balance: 0, clock: Self::now() });
        }
        require!(attached >= cost, "attach the full price");

        self.tick();
        self.settle(&token_id, &listing.seller);
        self.tokens.internal_transfer(&listing.seller, &buyer, &token_id, None, Some("sold".to_string()));
        self.halve_age(&token_id);

        let price = listing.price;
        let to_vault = mul_div(price, self.params.sale_vault_bps as u128, BPS);
        let to_creator = mul_div(price, self.params.sale_creator_bps as u128, BPS);
        let to_seller = price - to_vault - to_creator;
        self.vault += to_vault;
        self.sales += 1;
        self.sale_volume += price;
        self.push_log("sold", &buyer, 0, price, Self::number_of(&token_id));

        Self::pay(listing.seller, to_seller);
        Self::pay(self.params.creator.clone(), to_creator);
        Self::refund(&buyer, attached - cost);
        U128(to_seller)
    }

    /// Move what a soul has earned into its owner's wallet. Returns the SABIR paid.
    pub fn collect(&mut self, token_id: TokenId) -> U128 {
        let who = env::predecessor_account_id();
        require!(self.owner_of(&token_id) == who, "only the owner can collect");
        self.tick();
        let paid = self.settle(&token_id, &who);
        if paid > 0 {
            self.push_log("collect", &who, paid, 0, Self::number_of(&token_id));
        }
        U128(paid)
    }

    /// Give up: the token burns, what it earned goes to the owner, the deposit comes back minus
    /// the tax and the tax stays in the vault. `reason` goes into the exit book:
    /// 0 unsaid, 1 needed the money, 2 the price moved, 3 bored, 4 found better, 5 lost trust.
    #[payable]
    pub fn break_soul(&mut self, token_id: TokenId, reason: Option<u8>) -> Broken {
        assert_one_yocto();
        let who = env::predecessor_account_id();
        require!(self.owner_of(&token_id) == who, "only the owner can break this soul");
        self.tick();
        let sabir = self.settle(&token_id, &who);
        self.listings.remove(&token_id);
        let soul = self.souls.remove(&token_id).unwrap_or_else(|| env::panic_str("no such soul"));
        self.burn_token(&token_id, &who);
        self.souls_broken += 1;

        let now = Self::now();
        let tax_bps = self.tax_after(now.saturating_sub(soul.clock));
        let tax = mul_div(soul.deposit, tax_bps as u128, BPS);
        let refund = soul.deposit - tax;
        self.deposits -= soul.deposit;
        self.vault += tax;
        self.taxes_paid += tax;

        let reason = reason.filter(|r| (*r as usize) < self.book.broken_by_reason.len()).unwrap_or(0);
        let lived = now.saturating_sub(soul.born);
        let bucket = match lived as u128 * 40 / self.params.full_patience_secs as u128 {
            0 => 0,
            1..=6 => 1,
            7..=19 => 2,
            20..=39 => 3,
            _ => 4,
        };
        let layer = self.layer_index(soul.waited_secs);
        self.book.broken_by_layer[layer] += 1;
        self.book.broken_by_reason[reason as usize] += 1;
        self.book.broken_by_days[bucket] += 1;

        self.push_log("broke", &who, sabir, refund, Self::number_of(&token_id));
        env::log_str(&format!(
            "EVENT_JSON:{}",
            json!({
                "standard": "sabir",
                "version": "1.0.0",
                "event": "soul_broken",
                "data": [{
                    "token_id": token_id,
                    "owner_id": who,
                    "layer": LAYERS[layer],
                    "waited_secs": soul.waited_secs,
                    "toll": U128(soul.toll),
                    "lived_secs": lived,
                    "reason": reason,
                    "tax": U128(tax),
                    "refund": U128(refund),
                }],
            })
        ));
        Self::pay(who, refund);
        Broken { refund: U128(refund), tax: U128(tax), sabir: U128(sabir) }
    }

    pub fn get_soul(&self, token_id: TokenId) -> Option<SoulView> {
        let owner = self.tokens.owner_by_id.get(&token_id)?;
        let soul = self.souls.get(&token_id)?;
        Some(self.soul_view(&token_id, owner, soul))
    }

    /// Souls for sale, at most 50 per call.
    pub fn get_listings(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<ListingView> {
        let from = from_index.unwrap_or(0) as usize;
        let take = limit.map(u64::from).unwrap_or(PAGE).min(PAGE) as usize;
        self.listings
            .iter()
            .skip(from)
            .take(take)
            .filter_map(|(id, listing)| {
                let soul = self.get_soul(id.clone())?;
                Some(ListingView { token_id: id.clone(), seller: listing.seller.clone(), price: U128(listing.price), soul })
            })
            .collect()
    }
}

impl Sabir {
    /// Mints the soul of a crossing. The storage was paid for when the ticket was bought.
    /// `tick` must have run first: this adds a living soul.
    pub(crate) fn mint_soul(
        &mut self,
        who: &AccountId,
        entered: u64,
        waited: u64,
        toll: u128,
        deposit: u128,
        sketch: Vec<u8>,
    ) -> TokenId {
        self.souls_minted += 1;
        let number = self.souls_minted;
        let token_id = number.to_string();

        let mut input = env::random_seed();
        input.extend_from_slice(who.as_bytes());
        input.extend_from_slice(&number.to_le_bytes());
        let hash = env::sha256(&input);
        let mut seed = [0u8; 16];
        seed.copy_from_slice(&hash[..16]);

        let layer = self.layer_index(waited);
        self.book.born_by_layer[layer] += 1;
        self.souls.insert(
            token_id.clone(),
            Soul {
                seed,
                crossed_by: who.clone(),
                born: Self::now(),
                clock: entered,
                earned_to: self.earning_now(),
                waited_secs: waited,
                toll,
                deposit,
                sales: 0,
                sketch,
            },
        );
        self.tokens.internal_mint_with_refund(token_id.clone(), who.clone(), None, None);
        NftMint { owner_id: who, token_ids: &[&token_id], memo: None }.emit();
        token_id
    }

    /// Pays everything a soul has earned to `owner` and returns it. `tick` must have run first.
    /// The tokens carry the soul's age, so an old soul pulls the owner's clock back by its weight.
    pub(crate) fn settle(&mut self, token_id: &TokenId, owner: &AccountId) -> u128 {
        let end = self.earning_now();
        let soul = self.souls.get_mut(token_id).unwrap_or_else(|| env::panic_str("no such soul"));
        let secs = end.saturating_sub(soul.earned_to);
        if secs == 0 {
            return 0;
        }
        soul.earned_to = end;
        let clock = soul.clock;
        self.uncollected_secs -= secs as u128;
        let amount = self.tokens_of(secs as u128);
        let holder = self.holders.get_mut(owner).unwrap_or_else(|| env::panic_str("the owner is not registered"));
        holder.clock = Self::blend_clock(holder.balance, holder.clock, amount, clock);
        holder.balance += amount;
        self.total_supply += amount;
        FtMint { owner_id: owner, amount: U128(amount), memo: Some("earned by waiting") }.emit();
        amount
    }

    /// A payout that fails goes back to the vault.
    fn pay(to: AccountId, amount: u128) {
        if amount > 0 {
            Promise::new(to)
                .transfer(NearToken::from_yoctonear(amount))
                .then(Self::ext(env::current_account_id()).with_static_gas(GAS_ON_PAYOUT).on_payout(U128(amount)))
                .detach();
        }
    }

    /// NEAR the owner would get back by breaking the soul now.
    fn break_value(&self, soul: &Soul) -> u128 {
        let tax = self.tax_after(Self::now().saturating_sub(soul.clock));
        soul.deposit - mul_div(soul.deposit, tax as u128, BPS)
    }

    fn owner_of(&self, token_id: &TokenId) -> AccountId {
        self.tokens.owner_by_id.get(token_id).unwrap_or_else(|| env::panic_str("no such soul"))
    }

    fn number_of(token_id: &str) -> u64 {
        token_id.parse().unwrap_or(0)
    }

    /// Every sale costs a soul half of its age.
    fn halve_age(&mut self, token_id: &TokenId) {
        let now = Self::now();
        if let Some(soul) = self.souls.get_mut(token_id) {
            let age = now.saturating_sub(soul.clock);
            soul.clock = now - age / 2;
            soul.sales += 1;
        }
    }

    fn burn_token(&mut self, token_id: &TokenId, owner: &AccountId) {
        self.tokens.owner_by_id.remove(token_id);
        if let Some(per_owner) = &mut self.tokens.tokens_per_owner {
            let mut set = per_owner.get(owner).unwrap_or_else(|| env::panic_str("owner record is missing"));
            set.remove(token_id);
            if set.is_empty() {
                per_owner.remove(owner);
            } else {
                per_owner.insert(owner, &set);
            }
        }
        NftBurn { owner_id: owner, token_ids: &[token_id], authorized_id: None, memo: Some("broken") }.emit();
    }

    fn layer_index(&self, waited: u64) -> usize {
        let gate = self.params.gate_secs as u128;
        let at = waited as u128 * 5;
        if waited >= self.params.gate_secs {
            3
        } else if at < gate {
            0
        } else if at < gate * 3 {
            1
        } else {
            2
        }
    }

    fn soul_view(&self, token_id: &TokenId, owner_id: AccountId, soul: &Soul) -> SoulView {
        let age = Self::now().saturating_sub(soul.clock);
        let tax = self.tax_after(age);
        let earned = self.tokens_of(self.earning_now().saturating_sub(soul.earned_to) as u128);
        SoulView {
            token_id: token_id.clone(),
            owner_id,
            crossed_by: soul.crossed_by.clone(),
            seed: Self::hex(&soul.seed),
            layer: LAYERS[self.layer_index(soul.waited_secs)].to_string(),
            waited_secs: soul.waited_secs,
            toll: U128(soul.toll),
            born: soul.born,
            clock: soul.clock,
            age_secs: age,
            rank: self.rank_after(age).to_string(),
            deposit: U128(soul.deposit),
            tax_bps: tax,
            break_value: U128(self.break_value(soul)),
            earned: U128(earned),
            earned_value: U128(self.exit_value(earned, tax)),
            sales: soul.sales,
            price: self.listings.get(token_id).map(|l| U128(l.price)),
            sketch: Base64VecU8::from(soul.sketch.clone()),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(DIGITS[(b >> 4) as usize] as char);
            out.push(DIGITS[(b & 15) as usize] as char);
        }
        out
    }

    /// Token metadata is written when it is read, so the rank and the value stay current.
    fn dress(&self, mut token: Token) -> Token {
        if let Some(soul) = self.souls.get(&token.token_id) {
            let view = self.soul_view(&token.token_id, token.owner_id.clone(), soul);
            let extra = json!({
                "seed": view.seed,
                "layer": view.layer,
                "rank": view.rank,
                "waited_secs": view.waited_secs,
                "toll": view.toll,
                "deposit": view.deposit,
                "tax_bps": view.tax_bps,
                "break_value": view.break_value,
                "earned": view.earned,
                "earned_value": view.earned_value,
                "age_secs": view.age_secs,
                "sales": view.sales,
                "price": view.price,
                "crossed_by": view.crossed_by,
                "sketch": view.sketch,
            });
            token.metadata = Some(TokenMetadata {
                title: Some(format!("Soul #{}, {}", token.token_id, view.layer)),
                description: Some(format!(
                    "Crossed the SABIR gate after {} seconds. Rank: {}.",
                    view.waited_secs, view.rank
                )),
                media: Some(format!("{}{}.svg", self.params.media_base, token.token_id)),
                media_hash: None,
                copies: Some(1),
                issued_at: Some((soul.born * 1000).to_string()),
                expires_at: None,
                starts_at: None,
                updated_at: None,
                extra: Some(extra.to_string()),
                reference: None,
                reference_hash: None,
            });
        }
        token
    }
}

#[near]
impl NonFungibleTokenCore for Sabir {
    /// Closed on purpose: a free transfer plus a payment outside the contract would be a sale
    /// that skips the vault and the creator.
    #[payable]
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        let _ = (receiver_id, token_id, approval_id, memo);
        env::panic_str(SALE_ONLY);
    }

    /// Closed for the same reason as `nft_transfer`.
    #[payable]
    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        let _ = (receiver_id, token_id, approval_id, memo, msg);
        env::panic_str(SALE_ONLY)
    }

    fn nft_token(&self, token_id: TokenId) -> Option<Token> {
        self.tokens.nft_token(token_id).map(|t| self.dress(t))
    }
}

#[near]
impl NonFungibleTokenEnumeration for Sabir {
    fn nft_total_supply(&self) -> U128 {
        self.tokens.nft_total_supply()
    }

    fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        let limit = limit.unwrap_or(PAGE).min(PAGE);
        self.tokens.nft_tokens(from_index, Some(limit)).into_iter().map(|t| self.dress(t)).collect()
    }

    fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        self.tokens.nft_supply_for_owner(account_id)
    }

    fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        let limit = limit.unwrap_or(PAGE).min(PAGE);
        self.tokens.nft_tokens_for_owner(account_id, from_index, Some(limit)).into_iter().map(|t| self.dress(t)).collect()
    }
}

#[near]
impl NonFungibleTokenMetadataProvider for Sabir {
    fn nft_metadata(&self) -> NFTContractMetadata {
        NFTContractMetadata {
            spec: NFT_METADATA_SPEC.to_string(),
            name: "SABIR Souls".to_string(),
            symbol: "SOUL".to_string(),
            icon: Some(ICON.to_string()),
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }
}
