//! Regression test: a session rule only authorizes what its session key
//! signed.
//!
//! OZ's `do_check_auth` authenticates every signer the payload provides, but
//! for a rule with policies it leaves "did the rule's signers sign?" to the
//! policies' `enforce`. A policy that ignores `authenticated_signers` lets an
//! `AuthPayload` with an empty `signers` map, naming the session rule id,
//! drive the wallet without any key.
//!
//! This suite runs the real wallet and ed25519 verifier, the builds recorded
//! in `testdata/NOTICE.md`: the OpenZeppelin stellar-accounts v0.8.0 account
//! (`testdata/smart_account.wasm`, sha256 `c514d35e…b9e1`), which carries the
//! fix of advisory GHSA-jpg9-m7w4-c7wc, and the verifier
//! (`testdata/ed25519_verifier.wasm`, sha256 `60e8798d…b477`, the wasm of the
//! mainnet verifier `CBOOZV2B…UEMC`). It installs a session rule whose only
//! signer is a session key and whose policy is this contract, then authorizes
//! a market call with and without the key's signature. It also moves a real
//! token through the wallet to check the spend limit end to end.

extern crate std;

use ed25519_dalek::{Signer as _, SigningKey};
use fee_forwarder::{FeeForwarderContract, FeeForwarderContractClient};
use session_policy::{SessionPolicyContract, SessionPolicyContractClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::xdr::{
    Hash, HashIdPreimage, HashIdPreimageSorobanAuthorization, InvokeContractArgs, Limits,
    ScAddress, ScSymbol, ScVal, SorobanAddressCredentials, SorobanAuthorizationEntry,
    SorobanAuthorizedFunction, SorobanAuthorizedInvocation, SorobanCredentials, ToXdr as _,
    WriteXdr as _,
};
use soroban_sdk::{
    contract, contractimpl, vec, Address, Bytes, BytesN, Env, IntoVal, Map, String, Symbol,
    TryFromVal, Val, Vec,
};
use stellar_accounts::smart_account::{
    AuthDigestPreimage, AuthPayload, ContextRule, ContextRuleType, Signer,
};

const WALLET_WASM: &[u8] = include_bytes!("../testdata/smart_account.wasm");
const VERIFIER_WASM: &[u8] = include_bytes!("../testdata/ed25519_verifier.wasm");

/// A market whose `cancel_order` requires the wallet's authorization, like the
/// real one. The policy allows it on a configured market.
#[contract]
struct MockMarket;

#[contractimpl]
impl MockMarket {
    pub fn cancel_order(_e: Env, user: Address, id: u32) -> u32 {
        user.require_auth();
        id
    }

    /// Pulls `amount` of `token` from `user` into the market, as the real
    /// market escrows collateral.
    pub fn deposit(e: Env, user: Address, token: Address, amount: i128) {
        user.require_auth();
        TokenClient::new(&e, &token).transfer(&user, e.current_contract_address(), &amount);
    }
}

/// The session's spend limit: 100 tokens.
const LIMIT: i128 = 100 * 10_000_000;

struct World {
    e: Env,
    wallet: Address,
    verifier: Address,
    market: Address,
    token: Address,
    forwarder: Address,
    fee_recipient: Address,
    policy: Address,
    session_key: SigningKey,
    session_rule_id: u32,
}

/// Registers the wallet with an owner rule (rule 0, no policy) and installs a
/// session rule: the session key as its only signer, this policy attached.
fn setup() -> World {
    let e = Env::default();

    let verifier = e.register(VERIFIER_WASM, ());
    let owner = Address::generate(&e);
    let owner_signers: Vec<Signer> = vec![&e, Signer::Delegated(owner)];
    let wallet = e.register(WALLET_WASM, (owner_signers, Map::<Address, Val>::new(&e)));

    let market = e.register(MockMarket, ());
    let token = e
        .register_stellar_asset_contract_v2(Address::generate(&e))
        .address();
    let forwarder = e.register(FeeForwarderContract, ());
    let fee_recipient = Address::generate(&e);
    let policy = e.register(
        SessionPolicyContract,
        (
            forwarder.clone(),
            vec![&e, market.clone()],
            token.clone(),
            fee_recipient.clone(),
        ),
    );

    let session_key = SigningKey::from_bytes(&[9u8; 32]);

    // The wallet gets 1,000 tokens; only this call and the owner's are mocked.
    e.mock_all_auths();
    StellarAssetClient::new(&e, &token).mint(&wallet, &(10 * LIMIT));
    e.set_auths(&[]);

    let mut w = World {
        e,
        wallet,
        verifier,
        market,
        token,
        forwarder,
        fee_recipient,
        policy,
        session_key,
        session_rule_id: 0,
    };
    w.session_rule_id = add_session_rule(&w, LIMIT);
    w
}

/// The owner adds a session rule with the session key as its only signer and
/// the policy installed with `limit`. Returns the rule's id.
fn add_session_rule(w: &World, limit: i128) -> u32 {
    let e = &w.e;
    let session_signer = Signer::External(
        w.verifier.clone(),
        Bytes::from_array(e, &w.session_key.verifying_key().to_bytes()),
    );
    let mut policies: Map<Address, Val> = Map::new(e);
    policies.set(w.policy.clone(), limit.into_val(e));
    let valid_until: Option<u32> = Some(e.ledger().sequence() + 100_000);

    e.mock_all_auths();
    let rule: ContextRule = e.invoke_contract(
        &w.wallet,
        &Symbol::new(e, "add_context_rule"),
        vec![
            e,
            ContextRuleType::Default.into_val(e),
            String::from_str(e, "Trading Session").into_val(e),
            valid_until.into_val(e),
            vec![e, session_signer].into_val(e),
            policies.into_val(e),
        ],
    );
    e.set_auths(&[]);
    rule.id
}

fn sc_address(e: &Env, address: &Address) -> ScAddress {
    match ScVal::try_from_val(e, &address.to_val()).unwrap() {
        ScVal::Address(address) => address,
        other => panic!("not an address: {other:?}"),
    }
}

/// The authorized invocation `market.cancel_order(wallet, id)`.
fn cancel_invocation(w: &World, id: u32) -> SorobanAuthorizedInvocation {
    let e = &w.e;
    let args: std::vec::Vec<ScVal> = [w.wallet.to_val(), id.into_val(e)]
        .iter()
        .map(|v| ScVal::try_from_val(e, v).unwrap())
        .collect();
    SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
            contract_address: sc_address(e, &w.market),
            function_name: ScSymbol("cancel_order".try_into().unwrap()),
            args: args.try_into().unwrap(),
        }),
        sub_invocations: std::vec::Vec::new().try_into().unwrap(),
    }
}

/// The authorized invocation `market.deposit(wallet, token, amount)` with the
/// `token.transfer(wallet, market, amount)` it makes.
fn deposit_invocation(w: &World, amount: i128) -> SorobanAuthorizedInvocation {
    let e = &w.e;
    let scvals = |vals: &[Val]| -> std::vec::Vec<ScVal> {
        vals.iter()
            .map(|v| ScVal::try_from_val(e, v).unwrap())
            .collect()
    };
    let transfer = SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
            contract_address: sc_address(e, &w.token),
            function_name: ScSymbol("transfer".try_into().unwrap()),
            args: scvals(&[w.wallet.to_val(), w.market.to_val(), amount.into_val(e)])
                .try_into()
                .unwrap(),
        }),
        sub_invocations: std::vec::Vec::new().try_into().unwrap(),
    };
    SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
            contract_address: sc_address(e, &w.market),
            function_name: ScSymbol("deposit".try_into().unwrap()),
            args: scvals(&[w.wallet.to_val(), w.token.to_val(), amount.into_val(e)])
                .try_into()
                .unwrap(),
        }),
        sub_invocations: std::vec![transfer].try_into().unwrap(),
    }
}

/// The authorized invocation `contract.fn_name(args)` with the invocations it
/// makes.
fn invocation(
    w: &World,
    contract: &Address,
    fn_name: &str,
    args: &[Val],
    sub_invocations: std::vec::Vec<SorobanAuthorizedInvocation>,
) -> SorobanAuthorizedInvocation {
    let e = &w.e;
    let args: std::vec::Vec<ScVal> = args
        .iter()
        .map(|v| ScVal::try_from_val(e, v).unwrap())
        .collect();
    SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
            contract_address: sc_address(e, contract),
            function_name: ScSymbol(fn_name.try_into().unwrap()),
            args: args.try_into().unwrap(),
        }),
        sub_invocations: sub_invocations.try_into().unwrap(),
    }
}

/// A market call relayed through the forwarder.
struct Relay<'a> {
    /// The market function and its arguments.
    target_fn: &'a str,
    target_args: Vec<Val>,
    /// The market call's authorized invocation and its number of contexts.
    target: SorobanAuthorizedInvocation,
    target_contexts: u32,
    fee: i128,
    max_fee: i128,
    fee_recipient: Address,
}

/// Relays `relay` signed by the session key, and returns whether it went
/// through. The wallet authorizes the forward, the fee allowance the
/// forwarder sets, and the market call.
fn relay(w: &World, relay: Relay, nonce: i64) -> bool {
    let e = &w.e;
    let expiration = e.ledger().sequence() + 100;
    let target_fn = Symbol::new(e, relay.target_fn);
    let approve = invocation(
        w,
        &w.token,
        "approve",
        &[
            w.wallet.to_val(),
            w.forwarder.to_val(),
            relay.max_fee.into_val(e),
            expiration.into_val(e),
        ],
        std::vec![],
    );
    let root = invocation(
        w,
        &w.forwarder,
        "forward",
        &[
            w.token.to_val(),
            relay.max_fee.into_val(e),
            expiration.into_val(e),
            relay.fee_recipient.to_val(),
            w.market.to_val(),
            target_fn.to_val(),
            relay.target_args.to_val(),
        ],
        std::vec![approve, relay.target],
    );
    // The forward, the approve and the market call's own contexts.
    let payload = session_payload_for(w, &root, nonce, expiration, 2 + relay.target_contexts);
    e.set_auths(&[entry(w, root, payload, nonce, expiration)]);
    FeeForwarderContractClient::new(e, &w.forwarder)
        .try_forward(
            &w.token,
            &relay.fee,
            &relay.max_fee,
            &expiration,
            &w.market,
            &target_fn,
            &relay.target_args,
            &w.wallet,
            &relay.fee_recipient,
        )
        .is_ok()
}

/// A relayed `market.deposit(wallet, token, amount)` paying `fee` of at most
/// `max_fee` to the configured recipient.
fn relayed_deposit(w: &World, amount: i128, fee: i128, max_fee: i128) -> Relay<'static> {
    let e = &w.e;
    Relay {
        target_fn: "deposit",
        target_args: vec![e, w.wallet.to_val(), w.token.to_val(), amount.into_val(e)],
        target: deposit_invocation(w, amount),
        target_contexts: 2,
        fee,
        max_fee,
        fee_recipient: w.fee_recipient.clone(),
    }
}

/// Deposits `amount` signed by the session key, and returns whether the
/// wallet authorized it.
fn deposit(w: &World, amount: i128, nonce: i64) -> bool {
    let expiration = w.e.ledger().sequence() + 100;
    let root = deposit_invocation(w, amount);
    // Two contexts: the deposit and the transfer it makes.
    let payload = session_payload_for(w, &root, nonce, expiration, 2);
    w.e.set_auths(&[entry(w, root, payload, nonce, expiration)]);
    let client = MockMarketClient::new(&w.e, &w.market);
    client.try_deposit(&w.wallet, &w.token, &amount).is_ok()
}

/// The wallet's authorization entry over `root`, carrying `payload`.
fn entry(
    w: &World,
    root: SorobanAuthorizedInvocation,
    payload: AuthPayload,
    nonce: i64,
    expiration: u32,
) -> SorobanAuthorizationEntry {
    let e = &w.e;
    let signature: Val = payload.into_val(e);
    SorobanAuthorizationEntry {
        credentials: SorobanCredentials::Address(SorobanAddressCredentials {
            address: sc_address(e, &w.wallet),
            nonce,
            signature_expiration_ledger: expiration,
            signature: ScVal::try_from_val(e, &signature).unwrap(),
        }),
        root_invocation: root,
    }
}

/// The host's signature payload for the wallet's entry over `root`.
fn payload_hash(
    w: &World,
    root: &SorobanAuthorizedInvocation,
    nonce: i64,
    expiration: u32,
) -> BytesN<32> {
    let e = &w.e;
    let preimage = HashIdPreimage::SorobanAuthorization(HashIdPreimageSorobanAuthorization {
        network_id: Hash(e.ledger().network_id().to_array()),
        nonce,
        signature_expiration_ledger: expiration,
        invocation: root.clone(),
    });
    e.crypto()
        .sha256(&Bytes::from_slice(
            e,
            &preimage.to_xdr(Limits::none()).unwrap(),
        ))
        .to_bytes()
}

/// An `AuthPayload` naming `rule_ids`, one per context, with the session
/// key's ed25519 signature over `digest`.
fn signed_payload(w: &World, digest: &BytesN<32>, rule_ids: Vec<u32>) -> AuthPayload {
    let e = &w.e;
    let signature = w.session_key.sign(&digest.to_array()).to_bytes();
    let signer = Signer::External(
        w.verifier.clone(),
        Bytes::from_array(e, &w.session_key.verifying_key().to_bytes()),
    );
    let mut signers: Map<Signer, Bytes> = Map::new(e);
    signers.set(signer, Bytes::from_array(e, &signature));
    AuthPayload {
        signers,
        context_rule_ids: rule_ids,
    }
}

/// The session key's signature over `root`, as the wallet verifies it: the
/// ed25519 signature of the `AuthDigestPreimage` digest, which binds the
/// wallet's address, the host's payload and the rule ids.
fn session_payload(
    w: &World,
    root: &SorobanAuthorizedInvocation,
    nonce: i64,
    expiration: u32,
) -> AuthPayload {
    session_payload_for(w, root, nonce, expiration, 1)
}

/// `session_payload` for a `root` of `contexts` invocations: the payload
/// names the session rule once per context.
fn session_payload_for(
    w: &World,
    root: &SorobanAuthorizedInvocation,
    nonce: i64,
    expiration: u32,
    contexts: u32,
) -> AuthPayload {
    let e = &w.e;
    let mut rule_ids: Vec<u32> = Vec::new(e);
    for _ in 0..contexts {
        rule_ids.push_back(w.session_rule_id);
    }
    let digest = AuthDigestPreimage {
        account: w.wallet.clone(),
        signature_payload: payload_hash(w, root, nonce, expiration),
        context_rule_ids: rule_ids.clone(),
    }
    .digest(e);
    signed_payload(w, &digest, rule_ids)
}

/// The same signature over the digest earlier account builds verified,
/// `sha256(payload_hash || xdr(context_rule_ids))`, which does not bind the
/// wallet's address.
fn legacy_session_payload(
    w: &World,
    root: &SorobanAuthorizedInvocation,
    nonce: i64,
    expiration: u32,
) -> AuthPayload {
    let e = &w.e;
    let rule_ids: Vec<u32> = vec![e, w.session_rule_id];
    let mut digest = payload_hash(w, root, nonce, expiration).to_bytes();
    digest.append(&rule_ids.clone().to_xdr(e));
    signed_payload(w, &e.crypto().sha256(&digest).to_bytes(), rule_ids)
}

/// Calls `market.cancel_order(wallet, id)` and returns the panic message if
/// authorization fails.
fn cancel(w: &World, id: u32) -> Result<u32, std::string::String> {
    let client = MockMarketClient::new(&w.e, &w.market);
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.cancel_order(&w.wallet, &id)
    }))
    .map_err(|payload| {
        payload
            .downcast_ref::<std::string::String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|s| std::string::String::from(*s))
            })
            .unwrap_or_default()
    })
}

#[test]
fn signed_session_key_authorizes_a_trade() {
    let w = setup();
    let expiration = w.e.ledger().sequence() + 100;
    let root = cancel_invocation(&w, 1);
    let payload = session_payload(&w, &root, 1, expiration);
    w.e.set_auths(&[entry(&w, root, payload, 1, expiration)]);
    assert_eq!(cancel(&w, 1), Ok(1));
}

#[test]
fn session_key_signature_over_the_legacy_digest_is_rejected() {
    // The account verifies the `AuthDigestPreimage` digest only. A client that
    // still signs the earlier digest gets no authorization.
    let w = setup();
    let expiration = w.e.ledger().sequence() + 100;
    let root = cancel_invocation(&w, 1);
    let payload = legacy_session_payload(&w, &root, 1, expiration);
    w.e.set_auths(&[entry(&w, root, payload, 1, expiration)]);
    assert!(cancel(&w, 1).is_err());
}

#[test]
fn missing_authorization_is_rejected() {
    let w = setup();
    w.e.set_auths(&[]);
    assert!(cancel(&w, 1).is_err());
}

#[test]
fn empty_signature_on_the_owner_rule_is_rejected() {
    // Rule 0 has no policy, so the wallet itself requires its signer.
    let w = setup();
    let expiration = w.e.ledger().sequence() + 100;
    let payload = AuthPayload {
        signers: Map::new(&w.e),
        context_rule_ids: vec![&w.e, 0u32],
    };
    w.e.set_auths(&[entry(&w, cancel_invocation(&w, 1), payload, 2, expiration)]);
    assert!(cancel(&w, 1).is_err());
}

#[test]
fn empty_signature_on_the_session_rule_is_rejected_by_the_policy() {
    // The bypass: no signature at all, naming the session rule id, over a
    // context the policy allows. The wallet defers to the policy, which must
    // reject it with `SignerNotAuthenticated` (4007).
    let w = setup();
    let expiration = w.e.ledger().sequence() + 100;
    let payload = AuthPayload {
        signers: Map::new(&w.e),
        context_rule_ids: vec![&w.e, w.session_rule_id],
    };
    w.e.set_auths(&[entry(&w, cancel_invocation(&w, 1), payload, 3, expiration)]);
    let error = cancel(&w, 1).expect_err("an empty signature must not authorize a trade");
    assert!(
        error.contains("Error(Contract, #4007)"),
        "expected the policy's SignerNotAuthenticated (4007), got: {error}"
    );
}

#[test]
fn session_key_spends_up_to_the_limit() {
    let w = setup();
    let token = TokenClient::new(&w.e, &w.token);
    let policy = SessionPolicyContractClient::new(&w.e, &w.policy);
    let unit = LIMIT / 100;

    // 60 of the 100 pass, another 60 would make 120.
    assert!(deposit(&w, 60 * unit, 1));
    assert!(!deposit(&w, 60 * unit, 2));
    assert_eq!(token.balance(&w.market), 60 * unit);
    // The refused deposit left the count alone, and 40 still fit.
    assert!(deposit(&w, 40 * unit, 3));
    assert!(!deposit(&w, 1, 4));
    assert_eq!(token.balance(&w.market), LIMIT);
    let spend = policy.get_spend(&w.wallet, &w.session_rule_id).unwrap();
    assert_eq!((spend.spent, spend.limit), (LIMIT, LIMIT));
}

#[test]
fn a_new_rule_gives_the_same_key_a_new_limit() {
    let mut w = setup();
    let token = TokenClient::new(&w.e, &w.token);
    let policy = SessionPolicyContractClient::new(&w.e, &w.policy);
    assert!(deposit(&w, LIMIT, 1));
    assert!(!deposit(&w, 1, 2));

    // The owner adds a second rule for the same key with a higher limit.
    let used_up = w.session_rule_id;
    w.session_rule_id = add_session_rule(&w, 2 * LIMIT);
    assert_ne!(w.session_rule_id, used_up);
    assert!(deposit(&w, 2 * LIMIT, 3));
    assert!(!deposit(&w, 1, 4));
    assert_eq!(token.balance(&w.market), 3 * LIMIT);

    // The used-up rule keeps its count.
    let spend = policy.get_spend(&w.wallet, &used_up).unwrap();
    assert_eq!((spend.spent, spend.limit), (LIMIT, LIMIT));
}

#[test]
fn relayed_trade_counts_the_deposit_and_the_maximum_fee() {
    let w = setup();
    let token = TokenClient::new(&w.e, &w.token);
    let policy = SessionPolicyContractClient::new(&w.e, &w.policy);
    let spent = || {
        policy
            .get_spend(&w.wallet, &w.session_rule_id)
            .unwrap()
            .spent
    };
    let unit = LIMIT / 100;

    // A deposit of 50 with a fee of 1 under a maximum of 3: the wallet pays
    // 51 and the limit counts 53.
    assert!(relay(&w, relayed_deposit(&w, 50 * unit, unit, 3 * unit), 1));
    assert_eq!(token.balance(&w.market), 50 * unit);
    assert_eq!(token.balance(&w.fee_recipient), unit);
    assert_eq!(token.balance(&w.wallet), 10 * LIMIT - 51 * unit);
    assert_eq!(token.allowance(&w.wallet, &w.forwarder), 0);
    assert_eq!(spent(), 53 * unit);

    // 47 are left: a deposit of 47 with a maximum fee of 1 makes 48.
    assert!(!relay(&w, relayed_deposit(&w, 47 * unit, unit, unit), 2));
    assert_eq!(token.balance(&w.market), 50 * unit);
    assert_eq!(token.balance(&w.fee_recipient), unit);
    assert_eq!(spent(), 53 * unit);

    assert!(relay(&w, relayed_deposit(&w, 46 * unit, unit, unit), 3));
    assert_eq!(token.balance(&w.market), 96 * unit);
    assert_eq!(token.balance(&w.fee_recipient), 2 * unit);
    assert_eq!(spent(), LIMIT);
}

#[test]
fn used_up_key_cannot_pay_a_relay_fee() {
    let w = setup();
    let e = &w.e;
    let token = TokenClient::new(e, &w.token);
    assert!(deposit(&w, LIMIT, 1));

    // A relayed cancel moves no collateral, but its fee allowance counts.
    let relayed_cancel = Relay {
        target_fn: "cancel_order",
        target_args: vec![e, w.wallet.to_val(), 7u32.into_val(e)],
        target: cancel_invocation(&w, 7),
        target_contexts: 1,
        fee: 1,
        max_fee: 1,
        fee_recipient: w.fee_recipient.clone(),
    };
    assert!(!relay(&w, relayed_cancel, 2));
    assert_eq!(token.balance(&w.fee_recipient), 0);

    // The same cancel sent directly spends nothing and passes.
    let expiration = e.ledger().sequence() + 100;
    let root = cancel_invocation(&w, 7);
    let payload = session_payload(&w, &root, 3, expiration);
    e.set_auths(&[entry(&w, root, payload, 3, expiration)]);
    assert_eq!(cancel(&w, 7), Ok(7));
}

#[test]
fn relayed_trade_cannot_pay_another_recipient() {
    let w = setup();
    let token = TokenClient::new(&w.e, &w.token);
    let policy = SessionPolicyContractClient::new(&w.e, &w.policy);
    let attacker = Address::generate(&w.e);
    let unit = LIMIT / 100;

    let mut stolen = relayed_deposit(&w, unit, unit, unit);
    stolen.fee_recipient = attacker.clone();
    assert!(!relay(&w, stolen, 1));
    assert_eq!(token.balance(&attacker), 0);
    assert_eq!(token.balance(&w.market), 0);
    let spend = policy.get_spend(&w.wallet, &w.session_rule_id).unwrap();
    assert_eq!(spend.spent, 0);
}
