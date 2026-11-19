#![no_std]

//! Zenex guardian contract.
//!
//! The guardian is the owner of the contracts it controls: a market directly,
//! or the governance timelock that owns a market. Its own owner forwards any
//! call to them, and grants the `guardian` role. A holder halts a market: it
//! moves it to a stricter operational status, from Active (0) to OnIce (1) or
//! Frozen (2), or from OnIce to Frozen, and nothing else.
//!
//! Resuming, delisting and retiring a market stay with the owner.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, panic_with_error, symbol_short, Address,
    Env, IntoVal, Symbol, Val, Vec,
};
use stellar_access::access_control;
use stellar_access::ownable::{self, Ownable};
use stellar_macros::only_owner;

/// The guardian owns the 82x domain. `Unauthorized` publishes as discriminant
/// 1, outside that domain, to mirror the shared access-control convention.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GuardianError {
    /// The caller does not hold the `guardian` role.
    Unauthorized = 1,
    /// The status is not operational, or not stricter than the current one.
    InvalidStatus = 820,
    /// The role is not `guardian`.
    InvalidRole = 821,
    /// The guardian has no owner: its ownership was renounced.
    NoOwner = 822,
}

/// A role holder set the status of the market `target`.
#[contractevent]
#[derive(Clone)]
pub struct StatusSet {
    #[topic]
    pub target: Address,
    /// The status the market was set to: 1 (OnIce) or 2 (Frozen).
    pub status: u32,
    pub caller: Address,
}

/// The owner forwarded a call to `target`.
#[contractevent]
#[derive(Clone)]
pub struct Forwarded {
    #[topic]
    pub target: Address,
    pub fn_name: Symbol,
}

/// Ledgers per day, assuming ~5 s per ledger.
const ONE_DAY_LEDGERS: u32 = 17280;

/// Extends the instance to ~61 days once it has less than ~60 days left.
fn extend_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(ONE_DAY_LEDGERS * 60, ONE_DAY_LEDGERS * 61);
}

/// The role that halts a market: moves it to a stricter status.
const GUARDIAN_ROLE: Symbol = symbol_short!("guardian");

/// Checks that `caller` authorized the call and holds the `guardian` role, that
/// the guardian has an owner, and that `status` is a stricter operational
/// status for the market `target`.
fn require_halt(e: &Env, caller: &Address, target: &Address, status: u32) {
    caller.require_auth();
    if ownable::get_owner(e).is_none() {
        panic_with_error!(e, GuardianError::NoOwner);
    }
    if access_control::has_role(e, caller, &GUARDIAN_ROLE).is_none() {
        panic_with_error!(e, GuardianError::Unauthorized);
    }
    let current: u32 = e.invoke_contract(target, &Symbol::new(e, "get_status"), Vec::new(e));
    // A Delisted (3) or Retired market fails here too: every status up to
    // Frozen (2) is at or below its current one.
    if status > 2 || status <= current {
        panic_with_error!(e, GuardianError::InvalidStatus);
    }
}

#[contract]
pub struct GuardianContract;

#[contractimpl]
impl GuardianContract {
    pub fn __constructor(e: Env, owner: Address) {
        ownable::set_owner(&e, &owner);
        extend_instance(&e);
    }

    /// Owner only. Calls `fn_name` on `target` with `args` at once and
    /// returns the result. This reaches every owner-only entry of the
    /// contracts the guardian owns, their ownership transfer included.
    #[only_owner]
    pub fn forward(e: Env, target: Address, fn_name: Symbol, args: Vec<Val>) -> Val {
        extend_instance(&e);
        let result = e.invoke_contract::<Val>(&target, &fn_name, args);
        Forwarded { target, fn_name }.publish(&e);
        result
    }

    /// Sets the stricter status `status` on the market `target`. `caller`
    /// must authorize the call and hold the `guardian` role. Fails once the guardian's
    /// ownership is renounced.
    pub fn set_status(e: Env, caller: Address, target: Address, status: u32) {
        extend_instance(&e);
        require_halt(&e, &caller, &target, status);

        // A market this guardian owns gets the status directly. Any other
        // owner is assumed to be the governance contract and gets the call.
        let owner: Option<Address> =
            e.invoke_contract(&target, &Symbol::new(&e, "get_owner"), Vec::new(&e));
        let set_status = Symbol::new(&e, "set_status");
        match owner {
            Some(governance) if governance != e.current_contract_address() => {
                let args: Vec<Val> = Vec::from_array(&e, [target.to_val(), status.into_val(&e)]);
                e.invoke_contract::<Val>(&governance, &set_status, args);
            }
            _ => {
                let args: Vec<Val> = Vec::from_array(&e, [status.into_val(&e)]);
                e.invoke_contract::<Val>(&target, &set_status, args);
            }
        }
        StatusSet {
            target,
            status,
            caller,
        }
        .publish(&e);
    }

    /// Owner only. Grants `account` the role `guardian`. A no-op if it
    /// holds the role already. Publishes OpenZeppelin's `role_granted`.
    pub fn grant_role(e: Env, account: Address, role: Symbol) {
        extend_instance(&e);
        let owner = ownable::enforce_owner_auth(&e);
        if role != GUARDIAN_ROLE {
            panic_with_error!(&e, GuardianError::InvalidRole);
        }
        access_control::grant_role_no_auth(&e, &account, &role, &owner);
    }

    /// Owner only. Revokes `role` from `account`, and fails if the account
    /// does not hold it. Publishes OpenZeppelin's `role_revoked`.
    pub fn revoke_role(e: Env, account: Address, role: Symbol) {
        extend_instance(&e);
        let owner = ownable::enforce_owner_auth(&e);
        access_control::revoke_role_no_auth(&e, &account, &role, &owner);
    }
}

#[contractimpl(contracttrait)]
impl Ownable for GuardianContract {}

#[cfg(test)]
mod test;
