#![no_std]

//! Wallet factory: deploys the pinned smart-account wasm at an address bound
//! to its constructor arguments. See README.md.

#[cfg(test)]
mod test;

use soroban_sdk::{
    contract, contractevent, contractimpl, symbol_short, unwrap::UnwrapOptimized, xdr::ToXdr,
    Address, BytesN, ContractExecutable, Env, IntoVal, Map, Symbol, Val, Vec,
};
use stellar_accounts::smart_account::Signer;

/// The smart-account wasm every wallet runs: a `BytesN<32>`.
const WASM_HASH: Symbol = symbol_short!("wasm");

const EXTEND_AMOUNT: u32 = 30 * 17280; // ~30 days
const TTL_THRESHOLD: u32 = EXTEND_AMOUNT - 17280; // refresh at ~29 days

/// Emitted once for each wallet `deploy` creates.
///
/// Topics are `("wallet_deployed", wallet)` and the data is the map
/// `{account_wasm_hash}`. The factory emits nothing else, so its events are
/// the list of its wallets. The wallet is a topic so that one wallet can be
/// looked up. The signers are not repeated here: the wallet stores them.
#[contractevent]
#[derive(Clone)]
pub struct WalletDeployed {
    #[topic]
    pub wallet: Address,
    pub account_wasm_hash: BytesN<32>,
}

#[contract]
pub struct WalletFactoryContract;

#[contractimpl]
impl WalletFactoryContract {
    /// Pins the smart-account wasm, which must already be uploaded. There is
    /// no way to change it: a new wasm is a new factory.
    pub fn __constructor(e: Env, account_wasm_hash: BytesN<32>) {
        e.storage().instance().set(&WASM_HASH, &account_wasm_hash);
    }

    /// Deploys a wallet with `(signers, policies)` as its constructor
    /// arguments and returns its address. The salt is the SHA-256 of their
    /// XDR, so the address commits to the wallet's signers: anyone may call
    /// this, and a second deploy of the same arguments fails.
    ///
    /// # Events
    ///
    /// * topics - `["wallet_deployed", wallet: Address]`
    /// * data - `{account_wasm_hash: BytesN<32>}`
    pub fn deploy(e: Env, signers: Vec<Signer>, policies: Map<Address, Val>) -> Address {
        let args: Vec<Val> = (signers.clone(), policies.clone()).into_val(&e);
        let salt = e.crypto().sha256(&args.to_xdr(&e)).to_bytes();
        let instance = e.storage().instance();
        instance.extend_ttl(TTL_THRESHOLD, EXTEND_AMOUNT);
        let wasm_hash: BytesN<32> = instance.get(&WASM_HASH).unwrap_optimized();
        let wallet = e.deployer().with_current_contract(salt).deploy_contract(
            ContractExecutable::Wasm(wasm_hash.clone()),
            (signers, policies),
        );
        WalletDeployed {
            wallet: wallet.clone(),
            account_wasm_hash: wasm_hash,
        }
        .publish(&e);
        wallet
    }

    /// Returns the address `deploy` creates for these arguments, whether or
    /// not the wallet exists yet.
    pub fn predict_address(e: Env, signers: Vec<Signer>, policies: Map<Address, Val>) -> Address {
        let args: Vec<Val> = (signers, policies).into_val(&e);
        let salt = e.crypto().sha256(&args.to_xdr(&e)).to_bytes();
        e.deployer().with_current_contract(salt).deployed_address()
    }
}
