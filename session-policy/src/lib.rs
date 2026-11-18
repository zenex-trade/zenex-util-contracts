#![no_std]

//! Session policy: limits a wallet's trading session key to the Zenex trade
//! flow, with relay fees paid only to the pinned recipient, and to the spend
//! limit the wallet set when it installed the policy. See README.md.

#[cfg(test)]
mod test;

use soroban_sdk::{
    auth::{Context, ContractContext},
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short,
    unwrap::UnwrapOptimized,
    Address, Env, Symbol, TryFromVal, Val, Vec,
};
use stellar_accounts::{
    policies::Policy,
    smart_account::{ContextRule, Signer},
};

// ==========================================
// Storage
// ==========================================

// The spending of each wallet and rule is a persistent `Spend` entry. The
// configuration sits in instance storage, one entry per value: the
// forwarder branch reads only the forwarder and the recipient, and every other
// context reads the market list once. The keys are short symbols, which are
// constants: building one costs no host call.

/// The fee forwarder the key's relayed trades go through: an `Address`.
pub(crate) const FORWARDER: Symbol = symbol_short!("forwarder");
/// The markets the key may trade on: a `Vec<Address>`.
pub(crate) const MARKETS: Symbol = symbol_short!("markets");
/// The collateral token, which is also the fee token: an `Address`.
pub(crate) const TOKEN: Symbol = symbol_short!("token");
/// The only account relay fees may go to: an `Address`.
pub(crate) const FEE_RECIPIENT: Symbol = symbol_short!("recipient");

/// The install parameter: what the session key may spend per period.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendLimit {
    /// The most the key may spend in one period, in the token's units.
    pub limit: i128,
    /// The length of a period in ledgers.
    pub period_ledgers: u32,
}

/// A wallet's limit under one rule and its spending in the current period.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spend {
    pub limit: i128,
    pub period_ledgers: u32,
    /// The ledger of the first spend of the current period. The period is
    /// over at `period_start + period_ledgers`, and `spent` counts as zero
    /// from then on.
    pub period_start: u32,
    /// The amount spent since `period_start`.
    pub spent: i128,
}

/// Persistent storage keys.
#[contracttype]
pub(crate) enum DataKey {
    /// The `Spend` of `(smart_account, context_rule_id)`.
    Spend(Address, u32),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum SessionPolicyError {
    // The context is not a call to an allowed contract.
    ContractNotAllowed = 4002,
    // A token function other than `transfer` or `approve`.
    FunctionNotAllowed = 4003,
    // A token transfer goes somewhere other than a market.
    TransferNotAllowed = 4004,
    // A token approval names a spender other than the forwarder.
    ApproveNotAllowed = 4005,
    // A forward's signed fee recipient is not the pinned recipient.
    ForwardNotAllowed = 4006,
    // The session key did not sign.
    SignerNotAuthenticated = 4007,
    // The amount would take the period's spending over the limit.
    SpendLimitExceeded = 4008,
    // A transfer amount or a maximum fee is missing or negative.
    InvalidAmount = 4009,
    // The limit is negative or the period is zero ledgers.
    InvalidLimit = 4010,
    // The wallet has no limit stored under the rule.
    NotInstalled = 4011,
}

// ==========================================
// Constants
// ==========================================

const EXTEND_AMOUNT: u32 = 30 * 17280; // ~30 days
const TTL_THRESHOLD: u32 = EXTEND_AMOUNT - 17280; // refresh at ~29 days

// ==========================================
// Contract
// ==========================================

#[contract]
pub struct SessionPolicyContract;

#[contractimpl]
impl SessionPolicyContract {
    /// Fixes the policy's forwarder, markets, token and fee recipient. There
    /// is no way to change them later: a new configuration is a new
    /// deployment.
    pub fn __constructor(
        e: Env,
        forwarder: Address,
        markets: Vec<Address>,
        token: Address,
        fee_recipient: Address,
    ) {
        let instance = e.storage().instance();
        instance.set(&FORWARDER, &forwarder);
        instance.set(&MARKETS, &markets);
        instance.set(&TOKEN, &token);
        instance.set(&FEE_RECIPIENT, &fee_recipient);
    }
}

#[contractimpl]
impl Policy for SessionPolicyContract {
    type AccountParams = SpendLimit;

    fn enforce(
        e: &Env,
        context: Context,
        authenticated_signers: Vec<Signer>,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        smart_account.require_auth();

        // Require the session key's signature.
        if authenticated_signers.is_empty() {
            panic_with_error!(e, SessionPolicyError::SignerNotAuthenticated);
        }

        e.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, EXTEND_AMOUNT);

        // Only contract calls. Contract creation matches nothing below.
        let Context::Contract(ContractContext {
            contract,
            fn_name,
            args,
        }) = context
        else {
            panic_with_error!(e, SessionPolicyError::ContractNotAllowed);
        };

        // The first matching branch decides. The forwarder branch reads only the
        // forwarder and the recipient; every other context reads the market
        // list once.
        let instance = e.storage().instance();
        let forwarder: Address = instance.get(&FORWARDER).unwrap_optimized();
        if contract == forwarder {
            // The root of a relayed trade. Its args are the signed arguments
            // `(fee_token, max_fee_amount, expiration_ledger, fee_recipient,
            // target_contract, target_fn, [target_args])`. The fee leaves the
            // wallet through the forwarder's own `transfer_from`, which never
            // reaches this policy, so the signed recipient is the one thing to
            // pin. Every call the forward makes that needs the wallet's
            // authorization is a context of its own.
            let recipient = args
                .get(3)
                .and_then(|val| Address::try_from_val(e, &val).ok());
            let fee_recipient: Address = instance.get(&FEE_RECIPIENT).unwrap_optimized();
            if recipient != Some(fee_recipient) {
                panic_with_error!(e, SessionPolicyError::ForwardNotAllowed);
            }
            // The forwarder pulls at most the signed `max_fee_amount`, so
            // that amount counts as spent, whatever fee the relayer sets.
            spend(e, smart_account, context_rule.id, args.get(1));
            return;
        }

        let markets: Vec<Address> = instance.get(&MARKETS).unwrap_optimized();

        let token: Address = instance.get(&TOKEN).unwrap_optimized();
        if contract == token {
            if fn_name == symbol_short!("transfer") {
                // `transfer(from, to, amount)`: only escrow into a market. A
                // muxed `to` does not decode as an address, so it fails here
                // too.
                let to = args
                    .get(1)
                    .and_then(|val| Address::try_from_val(e, &val).ok());
                if !to.is_some_and(|to| markets.contains(&to)) {
                    panic_with_error!(e, SessionPolicyError::TransferNotAllowed);
                }
                spend(e, smart_account, context_rule.id, args.get(2));
            } else if fn_name == symbol_short!("approve") {
                // `approve(from, spender, amount, expiration_ledger)`: only the
                // forwarder's fee allowance, at any amount. The forwarder uses
                // it only under a signed forward, which is where the fee
                // counts as spent.
                let spender = args
                    .get(1)
                    .and_then(|val| Address::try_from_val(e, &val).ok());
                if spender != Some(forwarder) {
                    panic_with_error!(e, SessionPolicyError::ApproveNotAllowed);
                }
            } else {
                panic_with_error!(e, SessionPolicyError::FunctionNotAllowed);
            }
            return;
        }

        // A market call that needs the wallet's authorization acts on the
        // wallet's own funds and pays back to the wallet, so any function is
        // allowed.
        if markets.contains(&contract) {
            return;
        }

        panic_with_error!(e, SessionPolicyError::ContractNotAllowed);
    }

    fn install(
        e: &Env,
        params: Self::AccountParams,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        smart_account.require_auth();

        if params.limit < 0 || params.period_ledgers == 0 {
            panic_with_error!(e, SessionPolicyError::InvalidLimit);
        }
        let key = DataKey::Spend(smart_account, context_rule.id);
        let persistent = e.storage().persistent();
        persistent.set(
            &key,
            &Spend {
                limit: params.limit,
                period_ledgers: params.period_ledgers,
                period_start: 0,
                spent: 0,
            },
        );
        persistent.extend_ttl(&key, TTL_THRESHOLD, EXTEND_AMOUNT);
    }

    fn uninstall(e: &Env, context_rule: ContextRule, smart_account: Address) {
        smart_account.require_auth();
        e.storage()
            .persistent()
            .remove(&DataKey::Spend(smart_account, context_rule.id));
    }
}

#[contractimpl]
impl SessionPolicyContract {
    /// The limit and the spending of `smart_account` under the rule
    /// `context_rule_id`, or `None` if the policy is not installed there.
    pub fn get_spend(e: Env, smart_account: Address, context_rule_id: u32) -> Option<Spend> {
        e.storage()
            .persistent()
            .get(&DataKey::Spend(smart_account, context_rule_id))
    }
}

/// Counts `amount` against the limit of `smart_account` under the rule
/// `context_rule_id`. Fails if the amount is missing or negative, or if it
/// takes the period's spending over the limit.
fn spend(e: &Env, smart_account: Address, context_rule_id: u32, amount: Option<Val>) {
    let amount = amount
        .and_then(|val| i128::try_from_val(e, &val).ok())
        .filter(|amount| *amount >= 0)
        .unwrap_or_else(|| panic_with_error!(e, SessionPolicyError::InvalidAmount));

    let key = DataKey::Spend(smart_account, context_rule_id);
    let persistent = e.storage().persistent();
    let mut spend: Spend = persistent
        .get(&key)
        .unwrap_or_else(|| panic_with_error!(e, SessionPolicyError::NotInstalled));

    // A period starts at its first spend and ends `period_ledgers` later.
    let ledger = e.ledger().sequence();
    if spend.spent == 0 || ledger - spend.period_start >= spend.period_ledgers {
        spend.period_start = ledger;
        spend.spent = 0;
    }
    spend.spent = spend
        .spent
        .checked_add(amount)
        .filter(|spent| *spent <= spend.limit)
        .unwrap_or_else(|| panic_with_error!(e, SessionPolicyError::SpendLimitExceeded));

    persistent.set(&key, &spend);
    persistent.extend_ttl(&key, TTL_THRESHOLD, EXTEND_AMOUNT);
}
