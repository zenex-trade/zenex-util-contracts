#![no_std]

//! Wallet factory: deploys the current smart-account wasm at an address bound
//! to its constructor arguments. See README.md.

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractevent, contractimpl, symbol_short, unwrap::UnwrapOptimized, vec, xdr::ToXdr,
    Address, BytesN, ContractExecutable, Env, Map, Symbol, Val,
};
use stellar_access::ownable::{self, Ownable};
use stellar_accounts::smart_account::Signer;
use stellar_macros::only_owner;

/// The smart-account wasm new wallets are deployed with: a `BytesN<32>`.
const WASM_HASH: Symbol = symbol_short!("wasm");

const EXTEND_AMOUNT: u32 = 30 * 17280; // ~30 days
const TTL_THRESHOLD: u32 = EXTEND_AMOUNT - 17280; // refresh at ~29 days

/// Emitted for each wallet `deploy` creates, with the wasm it was created with.
#[contractevent]
#[derive(Clone)]
pub struct WalletDeployed {
    #[topic]
    pub wallet: Address,
    pub account_wasm_hash: BytesN<32>,
}

/// Emitted when the owner sets the wasm new wallets are deployed with.
#[contractevent]
#[derive(Clone)]
pub struct WasmSet {
    pub account_wasm_hash: BytesN<32>,
}

fn salt(e: &Env, signer: &Signer) -> BytesN<32> {
    e.crypto().sha256(&signer.clone().to_xdr(e)).to_bytes()
}

#[contract]
pub struct WalletFactoryContract;

#[contractimpl]
impl WalletFactoryContract {
    /// Sets the owner and the wasm new wallets start with, which must already
    /// be uploaded.
    pub fn __constructor(e: Env, owner: Address, account_wasm_hash: BytesN<32>) {
        ownable::set_owner(&e, &owner);
        e.storage().instance().set(&WASM_HASH, &account_wasm_hash);
    }

    /// Owner only. Sets the wasm new wallets are deployed with, which must
    /// already be uploaded. Wallets that exist keep their code: each upgrades
    /// itself.
    #[only_owner]
    pub fn set_wasm(e: Env, account_wasm_hash: BytesN<32>) {
        let instance = e.storage().instance();
        instance.extend_ttl(TTL_THRESHOLD, EXTEND_AMOUNT);
        instance.set(&WASM_HASH, &account_wasm_hash);
        WasmSet { account_wasm_hash }.publish(&e);
    }

    /// Deploys the wallet of `signer`, with that one signer and no policies,
    /// and returns its address. A signer has one wallet under this factory.
    pub fn deploy(e: Env, signer: Signer) -> Address {
        let instance = e.storage().instance();
        instance.extend_ttl(TTL_THRESHOLD, EXTEND_AMOUNT);
        let wasm_hash: BytesN<32> = instance.get(&WASM_HASH).unwrap_optimized();
        let policies: Map<Address, Val> = Map::new(&e);
        let wallet = e
            .deployer()
            .with_current_contract(salt(&e, &signer))
            .deploy_contract(
                ContractExecutable::Wasm(wasm_hash.clone()),
                (vec![&e, signer], policies),
            );
        WalletDeployed {
            wallet: wallet.clone(),
            account_wasm_hash: wasm_hash,
        }
        .publish(&e);
        wallet
    }

    /// Returns the address `deploy` creates for `signer`.
    pub fn predict_address(e: Env, signer: Signer) -> Address {
        e.deployer()
            .with_current_contract(salt(&e, &signer))
            .deployed_address()
    }
}

#[contractimpl(contracttrait)]
impl Ownable for WalletFactoryContract {}
