extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    vec, Address, Bytes, BytesN, Env, Event as _, IntoVal, Map, Symbol, Val, Vec,
};
use stellar_accounts::smart_account::{ContextRule, Signer};

use crate::{WalletDeployed, WalletFactoryContract, WalletFactoryContractClient, WasmSet};

/// The smart-account wasm (OpenZeppelin stellar-accounts v0.8.0) and the
/// ed25519 verifier it canonicalizes external keys with. Their sources are
/// recorded in `session-policy/testdata/NOTICE.md`.
const WALLET_WASM: &[u8] = include_bytes!("../../session-policy/testdata/smart_account.wasm");
const VERIFIER_WASM: &[u8] = include_bytes!("../../session-policy/testdata/ed25519_verifier.wasm");

fn account_wasm_hash(e: &Env) -> BytesN<32> {
    e.deployer()
        .upload_contract_wasm(Bytes::from_slice(e, WALLET_WASM))
}

/// Another uploaded wasm, to stand for a later account build.
fn other_wasm_hash(e: &Env) -> BytesN<32> {
    e.deployer()
        .upload_contract_wasm(Bytes::from_slice(e, VERIFIER_WASM))
}

fn setup(e: &Env) -> (WalletFactoryContractClient<'_>, Address) {
    let (factory, verifier, _) = setup_owned(e);
    (factory, verifier)
}

/// The factory, a verifier and the factory's owner. No call is authorized
/// unless a test mocks it.
fn setup_owned(e: &Env) -> (WalletFactoryContractClient<'_>, Address, Address) {
    let owner = Address::generate(e);
    let factory = e.register(WalletFactoryContract, (owner.clone(), account_wasm_hash(e)));
    let verifier = e.register(VERIFIER_WASM, ());
    (
        WalletFactoryContractClient::new(e, &factory),
        verifier,
        owner,
    )
}

/// The factory client with one call of `$fn_name` authorized as `$who`.
macro_rules! as_caller {
    ($factory:expr, $who:expr, $fn_name:literal, $args:expr $(,)?) => {
        $factory.mock_auths(&[MockAuth {
            address: $who,
            invoke: &MockAuthInvoke {
                contract: &$factory.address,
                fn_name: $fn_name,
                args: $args,
                sub_invokes: &[],
            },
        }])
    };
}

/// An external signer like the backend's passkey: a verifier plus key data.
fn signer(e: &Env, verifier: &Address, byte: u8) -> Vec<Signer> {
    vec![
        e,
        Signer::External(verifier.clone(), Bytes::from_array(e, &[byte; 32])),
    ]
}

#[test]
fn deploy_creates_the_wallet_at_the_predicted_address() {
    let e = Env::default();
    let (factory, verifier) = setup(&e);
    let signers = signer(&e, &verifier, 1);
    let policies = Map::<Address, Val>::new(&e);

    let predicted = factory.predict_address(&signers, &policies);
    let wallet = factory.deploy(&signers, &policies);
    assert_eq!(wallet, predicted);

    // The wallet's default rule (id 0) holds exactly the passed signers.
    let rule: ContextRule = e.invoke_contract(
        &wallet,
        &Symbol::new(&e, "get_context_rule"),
        vec![&e, 0u32.into_val(&e)],
    );
    assert_eq!(rule.signers, signers);
}

#[test]
fn different_signers_give_a_different_address() {
    let e = Env::default();
    let (factory, verifier) = setup(&e);
    let policies = Map::<Address, Val>::new(&e);

    assert_ne!(
        factory.predict_address(&signer(&e, &verifier, 1), &policies),
        factory.predict_address(&signer(&e, &verifier, 2), &policies)
    );
}

#[test]
fn deploying_the_same_signers_twice_fails() {
    let e = Env::default();
    let (factory, verifier) = setup(&e);
    let signers = signer(&e, &verifier, 1);
    let policies = Map::<Address, Val>::new(&e);

    factory.deploy(&signers, &policies);
    assert!(factory.try_deploy(&signers, &policies).is_err());
}

#[test]
fn deploy_emits_one_event_with_the_wallet_and_the_wasm_hash() {
    let e = Env::default();
    let (factory, verifier) = setup(&e);
    let policies = Map::<Address, Val>::new(&e);
    // Read before the deploy: `events().all()` holds the last invocation's
    // events only.
    let wasm_hash = account_wasm_hash(&e);

    for byte in [1, 2] {
        let signers = signer(&e, &verifier, byte);
        let predicted = factory.predict_address(&signers, &policies);
        let wallet = factory.deploy(&signers, &policies);
        assert_eq!(wallet, predicted);

        let expected = WalletDeployed {
            wallet,
            account_wasm_hash: wasm_hash.clone(),
        }
        .to_xdr(&e, &factory.address);
        assert_eq!(
            e.events().all().filter_by_contract(&factory.address),
            [expected],
            "exactly one factory event, topics (\"wallet_deployed\", wallet)"
        );
    }
}

#[test]
fn a_failed_duplicate_deploy_emits_nothing() {
    let e = Env::default();
    let (factory, verifier) = setup(&e);
    let signers = signer(&e, &verifier, 1);
    let policies = Map::<Address, Val>::new(&e);

    factory.deploy(&signers, &policies);
    assert_eq!(
        e.events()
            .all()
            .filter_by_contract(&factory.address)
            .events()
            .len(),
        1
    );

    assert!(factory.try_deploy(&signers, &policies).is_err());
    assert!(e.events().all().events().is_empty());
}

#[test]
fn predict_address_emits_nothing() {
    let e = Env::default();
    let (factory, verifier) = setup(&e);

    factory.predict_address(&signer(&e, &verifier, 1), &Map::new(&e));
    assert!(e.events().all().events().is_empty());
}

#[test]
fn the_owner_sets_the_wasm_of_new_wallets() {
    let e = Env::default();
    let (factory, _, owner) = setup_owned(&e);
    let next = other_wasm_hash(&e);

    as_caller!(&factory, &owner, "set_wasm", (next.clone(),).into_val(&e)).set_wasm(&next);

    let expected = WasmSet {
        account_wasm_hash: next,
    }
    .to_xdr(&e, &factory.address);
    assert_eq!(
        e.events().all().filter_by_contract(&factory.address),
        [expected]
    );
}

#[test]
fn only_the_owner_sets_the_wasm() {
    let e = Env::default();
    let (factory, _, owner) = setup_owned(&e);
    let stranger = Address::generate(&e);
    let next = other_wasm_hash(&e);

    assert!(as_caller!(
        &factory,
        &stranger,
        "set_wasm",
        (next.clone(),).into_val(&e)
    )
    .try_set_wasm(&next)
    .is_err());

    // After the owner renounces, nobody can.
    as_caller!(&factory, &owner, "renounce_ownership", ().into_val(&e)).renounce_ownership();
    assert!(
        as_caller!(&factory, &owner, "set_wasm", (next.clone(),).into_val(&e))
            .try_set_wasm(&next)
            .is_err()
    );
}

#[test]
fn a_wallet_address_does_not_depend_on_the_wasm() {
    let e = Env::default();
    let (factory, verifier, owner) = setup_owned(&e);
    let signers = signer(&e, &verifier, 1);
    let policies = Map::<Address, Val>::new(&e);
    let before = factory.predict_address(&signers, &policies);
    assert_eq!(factory.deploy(&signers, &policies), before);

    let next = other_wasm_hash(&e);
    as_caller!(&factory, &owner, "set_wasm", (next.clone(),).into_val(&e)).set_wasm(&next);

    assert_eq!(factory.predict_address(&signers, &policies), before);
    // The same signers cannot get a second wallet under the new wasm.
    assert!(factory.try_deploy(&signers, &policies).is_err());
}
