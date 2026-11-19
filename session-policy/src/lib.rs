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

/// The fee forwarder the key's relayed trades go through: an `Address`.
pub(crate) const FORWARDER: Symbol = symbol_short!("forwarder");
/// The markets the key may trade on: a `Vec<Address>`.
pub(crate) const MARKETS: Symbol = symbol_short!("markets");
/// The collateral token, which is also the fee token: an `Address`.
pub(crate) const TOKEN: Symbol = symbol_short!("token");
/// The only account relay fees may go to: an `Address`.
pub(crate) const FEE_RECIPIENT: Symbol = symbol_short!("recipient");

/// A wallet's limit under one rule and its spending so far.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spend {
    /// The most the key may spend under the rule, in the token's units.
    pub limit: i128,
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
    // A forward's signed fee token or fee recipient is not the pinned one.
    ForwardNotAllowed = 4006,
    // The session key did not sign.
    SignerNotAuthenticated = 4007,
    // The amount would take the rule's spending over the limit.
    SpendLimitExceeded = 4008,
    // A transfer or approve amount is missing or negative.
    InvalidAmount = 4009,
    // The limit is negative.
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
    /// The spend limit of the rule, in the token's units.
    type AccountParams = i128;

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

        let instance = e.storage().instance();
        instance.extend_ttl(TTL_THRESHOLD, EXTEND_AMOUNT);

        // Contract creation is never allowed.
        let Context::Contract(ContractContext {
            contract,
            fn_name,
            args,
        }) = context
        else {
            panic_with_error!(e, SessionPolicyError::ContractNotAllowed);
        };

        let forwarder: Address = instance.get(&FORWARDER).unwrap_optimized();
        let token: Address = instance.get(&TOKEN).unwrap_optimized();

        if contract == forwarder {
            // Signed args: `(fee_token, max_fee_amount, expiration_ledger,
            // fee_recipient, ..)`.
            let fee_recipient: Address = instance.get(&FEE_RECIPIENT).unwrap_optimized();
            if address_arg(e, &args, 0) != Some(token)
                || address_arg(e, &args, 3) != Some(fee_recipient)
            {
                panic_with_error!(e, SessionPolicyError::ForwardNotAllowed);
            }
        } else if contract == token {
            if fn_name == symbol_short!("transfer") {
                // `transfer(from, to, amount)`: only into a market.
                if !address_arg(e, &args, 1).is_some_and(|to| markets(e).contains(to)) {
                    panic_with_error!(e, SessionPolicyError::TransferNotAllowed);
                }
            } else if fn_name == symbol_short!("approve") {
                // `approve(from, spender, amount, ..)`: only to the forwarder.
                if address_arg(e, &args, 1) != Some(forwarder) {
                    panic_with_error!(e, SessionPolicyError::ApproveNotAllowed);
                }
            } else {
                panic_with_error!(e, SessionPolicyError::FunctionNotAllowed);
            }
            // Both functions carry the amount at index 2.
            spend(e, smart_account, context_rule.id, args.get(2));
        } else if !markets(e).contains(&contract) {
            panic_with_error!(e, SessionPolicyError::ContractNotAllowed);
        }
    }

    fn install(
        e: &Env,
        limit: Self::AccountParams,
        context_rule: ContextRule,
        smart_account: Address,
    ) {
        smart_account.require_auth();

        if limit < 0 {
            panic_with_error!(e, SessionPolicyError::InvalidLimit);
        }
        let key = DataKey::Spend(smart_account, context_rule.id);
        let persistent = e.storage().persistent();
        persistent.set(&key, &Spend { limit, spent: 0 });
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

/// The markets the key may trade on.
fn markets(e: &Env) -> Vec<Address> {
    e.storage().instance().get(&MARKETS).unwrap_optimized()
}

/// The argument at `index` as an `Address`, or `None` if it is missing or not one.
fn address_arg(e: &Env, args: &Vec<Val>, index: u32) -> Option<Address> {
    args.get(index)
        .and_then(|val| Address::try_from_val(e, &val).ok())
}

/// Counts `amount` against the limit of `smart_account` under the rule
/// `context_rule_id`. Fails if the amount is missing or negative, or if it
/// takes the rule's spending over the limit.
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

    spend.spent = spend
        .spent
        .checked_add(amount)
        .filter(|spent| *spent <= spend.limit)
        .unwrap_or_else(|| panic_with_error!(e, SessionPolicyError::SpendLimitExceeded));

    persistent.set(&key, &spend);
    persistent.extend_ttl(&key, TTL_THRESHOLD, EXTEND_AMOUNT);
}
