use crate::{GuardianContract, GuardianContractClient as GuardianClient, GuardianError};
use soroban_sdk::testutils::{Address as _, Ledger as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Error, IntoVal, Symbol, Val, Vec,
};

const ACTIVE: u32 = 0;
const ON_ICE: u32 = 1;
const FROZEN: u32 = 2;
const DELISTED: u32 = 3;

#[contracttype]
enum MarketKey {
    Owner,
    Status,
    Config,
}

/// Stands in for the market: `set_status` and `set_config` need the owner's
/// authorization, the way the market's `#[only_owner]` entries do.
#[contract]
pub struct MockMarket;

#[contractimpl]
impl MockMarket {
    pub fn __constructor(e: Env, owner: Address) {
        e.storage().instance().set(&MarketKey::Owner, &owner);
    }

    fn require_owner(e: &Env) {
        let owner: Address = e.storage().instance().get(&MarketKey::Owner).unwrap();
        owner.require_auth();
    }

    pub fn set_status(e: Env, status: u32) {
        Self::require_owner(&e);
        e.storage().instance().set(&MarketKey::Status, &status);
    }

    pub fn set_config(e: Env, value: u32) -> u32 {
        Self::require_owner(&e);
        e.storage().instance().set(&MarketKey::Config, &value);
        value + 1
    }

    pub fn get_owner(e: Env) -> Option<Address> {
        e.storage().instance().get(&MarketKey::Owner)
    }

    pub fn get_status(e: Env) -> u32 {
        e.storage().instance().get(&MarketKey::Status).unwrap_or(0)
    }

    pub fn get_config(e: Env) -> u32 {
        e.storage().instance().get(&MarketKey::Config).unwrap_or(0)
    }
}

/// The released governance 1.0.0 contract, see `testdata/NOTICE.md`.
mod governance {
    soroban_sdk::contractimport!(file = "testdata/governance_v1.0.0.wasm");
}

/// The governance timelock delay in the tests (seconds).
const DELAY: u64 = 3600;

/// A guardian that owns the real governance contract, which owns a mock
/// market.
fn setup_governed<'a>() -> (Setup<'a>, Address) {
    let e = Env::default();
    e.mock_all_auths();
    let owner = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let governance_id = e.register(governance::WASM, (&guardian_id, DELAY));
    let market_id = e.register(MockMarket, (&governance_id,));
    let setup = Setup {
        guardian: GuardianClient::new(&e, &guardian_id),
        market: MockMarketClient::new(&e, &market_id),
        owner,
        e,
    };
    (setup, governance_id)
}

struct Setup<'a> {
    e: Env,
    owner: Address,
    guardian: GuardianClient<'a>,
    market: MockMarketClient<'a>,
}

/// A guardian that owns a mock market, with every authorization mocked.
fn setup<'a>() -> Setup<'a> {
    let e = Env::default();
    e.mock_all_auths();
    let owner = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let market_id = e.register(MockMarket, (&guardian_id,));
    Setup {
        guardian: GuardianClient::new(&e, &guardian_id),
        market: MockMarketClient::new(&e, &market_id),
        owner,
        e,
    }
}

fn guardian_error(error: GuardianError) -> Result<Error, soroban_sdk::InvokeError> {
    Ok(Error::from_contract_error(error as u32))
}

#[test]
fn owner_forwards_any_call_and_gets_the_result() {
    let s = setup();
    let args: Vec<Val> = Vec::from_array(&s.e, [41u32.into_val(&s.e)]);
    let result = s
        .guardian
        .forward(&s.market.address, &Symbol::new(&s.e, "set_config"), &args);
    assert_eq!(s.market.get_config(), 41);
    let returned: u32 = result.into_val(&s.e);
    assert_eq!(returned, 42);
}

#[test]
fn owner_forwards_a_delist() {
    let s = setup();
    let args: Vec<Val> = Vec::from_array(&s.e, [DELISTED.into_val(&s.e)]);
    s.guardian
        .forward(&s.market.address, &Symbol::new(&s.e, "set_status"), &args);
    assert_eq!(s.market.get_status(), DELISTED);
}

#[test]
fn forward_needs_the_owner() {
    let e = Env::default();
    let owner = Address::generate(&e);
    let stranger = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let market_id = e.register(MockMarket, (&guardian_id,));
    let guardian = GuardianClient::new(&e, &guardian_id);
    let market = MockMarketClient::new(&e, &market_id);
    let fn_name = Symbol::new(&e, "set_config");
    let args: Vec<Val> = Vec::from_array(&e, [7u32.into_val(&e)]);

    // The stranger signs the call. The guardian asks for the owner.
    let result = guardian
        .mock_auths(&[MockAuth {
            address: &stranger,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "forward",
                args: (&market_id, &fn_name, &args).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .try_forward(&market_id, &fn_name, &args);
    assert!(result.is_err());
    assert_eq!(market.get_config(), 0);

    // The owner's own signature passes, and the market accepts the guardian
    // as its owner without any further signature.
    guardian
        .mock_auths(&[MockAuth {
            address: &owner,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "forward",
                args: (&market_id, &fn_name, &args).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .forward(&market_id, &fn_name, &args);
    assert_eq!(market.get_config(), 7);
}

/// Sets the market status as the owner, through `forward`.
fn owner_sets(s: &Setup, status: u32) {
    let args: Vec<Val> = Vec::from_array(&s.e, [status.into_val(&s.e)]);
    s.guardian
        .forward(&s.market.address, &Symbol::new(&s.e, "set_status"), &args);
}

fn guardian_role(e: &Env) -> Symbol {
    Symbol::new(e, "guardian")
}

/// An account with the `guardian` role.
fn grant_monitor(s: &Setup) -> Address {
    let monitor = Address::generate(&s.e);
    s.guardian.grant_role(&monitor, &guardian_role(&s.e));
    monitor
}

#[test]
fn guardian_role_halts_a_market() {
    let s = setup();
    let monitor = grant_monitor(&s);

    s.guardian.set_status(&monitor, &s.market.address, &ON_ICE);
    assert_eq!(s.market.get_status(), ON_ICE);
    s.guardian.set_status(&monitor, &s.market.address, &FROZEN);
    assert_eq!(s.market.get_status(), FROZEN);

    // Straight from Active to Frozen works too.
    owner_sets(&s, ACTIVE);
    s.guardian.set_status(&monitor, &s.market.address, &FROZEN);
    assert_eq!(s.market.get_status(), FROZEN);
}

#[test]
fn guardian_role_never_loosens_a_status() {
    let s = setup();
    let monitor = grant_monitor(&s);

    s.guardian.set_status(&monitor, &s.market.address, &ON_ICE);
    assert_eq!(
        s.guardian
            .try_set_status(&monitor, &s.market.address, &ACTIVE),
        Err(guardian_error(GuardianError::InvalidStatus))
    );
    assert_eq!(s.market.get_status(), ON_ICE);

    s.guardian.set_status(&monitor, &s.market.address, &FROZEN);
    for status in [ON_ICE, ACTIVE] {
        assert_eq!(
            s.guardian
                .try_set_status(&monitor, &s.market.address, &status),
            Err(guardian_error(GuardianError::InvalidStatus))
        );
    }
    assert_eq!(s.market.get_status(), FROZEN);
}

#[test]
fn only_a_stricter_operational_status_is_valid() {
    let s = setup();
    let monitor = grant_monitor(&s);
    // Active already, and Delisted and beyond are not operational.
    for status in [ACTIVE, DELISTED, 4, 5] {
        assert_eq!(
            s.guardian
                .try_set_status(&monitor, &s.market.address, &status),
            Err(guardian_error(GuardianError::InvalidStatus))
        );
    }
    assert_eq!(s.market.get_status(), ACTIVE);
}

#[test]
fn the_role_does_not_move_a_delisted_market() {
    let s = setup();
    let monitor = grant_monitor(&s);
    owner_sets(&s, DELISTED);

    for status in [ACTIVE, ON_ICE, FROZEN] {
        assert_eq!(
            s.guardian
                .try_set_status(&monitor, &s.market.address, &status),
            Err(guardian_error(GuardianError::NotOperational))
        );
    }
    assert_eq!(s.market.get_status(), DELISTED);
}

#[test]
fn the_role_does_not_resume_a_frozen_delisted_market() {
    let s = setup();
    let monitor = grant_monitor(&s);
    owner_sets(&s, DELISTED);
    owner_sets(&s, FROZEN);

    for status in [ACTIVE, ON_ICE] {
        assert_eq!(
            s.guardian
                .try_set_status(&monitor, &s.market.address, &status),
            Err(guardian_error(GuardianError::InvalidStatus))
        );
    }
    assert_eq!(s.market.get_status(), FROZEN);
}

#[test]
fn the_owner_holds_no_role_by_default() {
    let s = setup();
    assert_eq!(
        s.guardian
            .try_set_status(&s.owner, &s.market.address, &FROZEN),
        Err(guardian_error(GuardianError::Unauthorized))
    );
    // It resumes through `forward`.
    owner_sets(&s, FROZEN);
    owner_sets(&s, ACTIVE);
    assert_eq!(s.market.get_status(), ACTIVE);
}

#[test]
fn set_status_needs_the_caller_signature() {
    let e = Env::default();
    let owner = Address::generate(&e);
    let monitor = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let market_id = e.register(MockMarket, (&guardian_id,));
    let guardian = GuardianClient::new(&e, &guardian_id);
    let market = MockMarketClient::new(&e, &market_id);
    let role = guardian_role(&e);

    guardian
        .mock_auths(&[MockAuth {
            address: &owner,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "grant_role",
                args: (&monitor, &role).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .grant_role(&monitor, &role);

    // Naming the monitor as caller without its signature fails.
    assert!(guardian
        .try_set_status(&monitor, &market_id, &FROZEN)
        .is_err());
    assert_eq!(market.get_status(), ACTIVE);

    guardian
        .mock_auths(&[MockAuth {
            address: &monitor,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "set_status",
                args: (&monitor, &market_id, FROZEN).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .set_status(&monitor, &market_id, &FROZEN);
    assert_eq!(market.get_status(), FROZEN);
}

#[test]
fn owner_grants_and_revokes_the_role() {
    let s = setup();
    let monitor = grant_monitor(&s);
    // Granting twice changes nothing.
    s.guardian.grant_role(&monitor, &guardian_role(&s.e));

    // Only `guardian` exists.
    for name in ["admin", "resume"] {
        let other = Symbol::new(&s.e, name);
        assert_eq!(
            s.guardian.try_grant_role(&monitor, &other),
            Err(guardian_error(GuardianError::InvalidRole))
        );
        assert_eq!(
            s.guardian.try_revoke_role(&monitor, &other),
            Err(guardian_error(GuardianError::InvalidRole))
        );
    }

    s.guardian.revoke_role(&monitor, &guardian_role(&s.e));
    assert_eq!(
        s.guardian
            .try_set_status(&monitor, &s.market.address, &FROZEN),
        Err(guardian_error(GuardianError::Unauthorized))
    );

    // Revoking a role the account does not hold fails.
    assert!(s
        .guardian
        .try_revoke_role(&monitor, &guardian_role(&s.e))
        .is_err());
}

#[test]
fn role_management_needs_the_owner() {
    let e = Env::default();
    let owner = Address::generate(&e);
    let stranger = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let guardian = GuardianClient::new(&e, &guardian_id);
    let role = guardian_role(&e);

    let result = guardian
        .mock_auths(&[MockAuth {
            address: &stranger,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "grant_role",
                args: (&stranger, &role).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .try_grant_role(&stranger, &role);
    assert!(result.is_err());

    // The stranger got no role: its own signature does not let it freeze.
    let market_id = e.register(MockMarket, (&guardian_id,));
    e.mock_all_auths();
    assert_eq!(
        guardian.try_set_status(&stranger, &market_id, &FROZEN),
        Err(guardian_error(GuardianError::Unauthorized))
    );
}

#[test]
fn the_role_halts_a_governed_market() {
    let (s, _governance) = setup_governed();
    let monitor = grant_monitor(&s);

    s.guardian.set_status(&monitor, &s.market.address, &FROZEN);
    assert_eq!(s.market.get_status(), FROZEN);

    // The role cannot resume through governance either.
    assert_eq!(
        s.guardian
            .try_set_status(&monitor, &s.market.address, &ACTIVE),
        Err(guardian_error(GuardianError::InvalidStatus))
    );
    assert_eq!(s.market.get_status(), FROZEN);
}

#[test]
fn a_market_owned_by_someone_else_does_not_move() {
    let e = Env::default();
    let owner = Address::generate(&e);
    let monitor = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let guardian = GuardianClient::new(&e, &guardian_id);
    let role = guardian_role(&e);
    guardian
        .mock_auths(&[MockAuth {
            address: &owner,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "grant_role",
                args: (&monitor, &role).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .grant_role(&monitor, &role);

    // A governance contract the guardian does not own rejects the call, with
    // the monitor's valid signature.
    let foreign_governance = e.register(governance::WASM, (&owner, DELAY));
    let market_id = e.register(MockMarket, (&foreign_governance,));
    let market = MockMarketClient::new(&e, &market_id);
    let monitor_auth = MockAuth {
        address: &monitor,
        invoke: &MockAuthInvoke {
            contract: &guardian_id,
            fn_name: "set_status",
            args: (&monitor, &market_id, FROZEN).into_val(&e),
            sub_invokes: &[],
        },
    };
    assert!(guardian
        .mock_auths(core::slice::from_ref(&monitor_auth))
        .try_set_status(&monitor, &market_id, &FROZEN)
        .is_err());
    assert_eq!(market.get_status(), ACTIVE);

    // The one thing missing was the authorization of that governance's
    // owner: with it the same call passes.
    let owner_auth = MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &foreign_governance,
            fn_name: "set_status",
            args: (&market_id, FROZEN).into_val(&e),
            sub_invokes: &[],
        },
    };
    guardian
        .mock_auths(&[monitor_auth, owner_auth])
        .set_status(&monitor, &market_id, &FROZEN);
    assert_eq!(market.get_status(), FROZEN);
}

#[test]
fn a_renounced_guardian_sets_no_status() {
    let s = setup();
    let monitor = grant_monitor(&s);
    s.guardian.renounce_ownership();
    assert_eq!(s.guardian.get_owner(), None);

    assert_eq!(
        s.guardian
            .try_set_status(&monitor, &s.market.address, &FROZEN),
        Err(guardian_error(GuardianError::NoOwner))
    );
    assert_eq!(s.market.get_status(), ACTIVE);
    // Nothing else works either.
    let args: Vec<Val> = Vec::from_array(&s.e, [FROZEN.into_val(&s.e)]);
    assert!(s
        .guardian
        .try_forward(&s.market.address, &Symbol::new(&s.e, "set_status"), &args)
        .is_err());
    assert!(s
        .guardian
        .try_grant_role(&monitor, &guardian_role(&s.e))
        .is_err());
}

#[test]
fn governed_halt_needs_only_the_caller_signature() {
    let e = Env::default();
    let owner = Address::generate(&e);
    let monitor = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let governance_id = e.register(governance::WASM, (&guardian_id, DELAY));
    let market_id = e.register(MockMarket, (&governance_id,));
    let guardian = GuardianClient::new(&e, &guardian_id);
    let market = MockMarketClient::new(&e, &market_id);
    let role = guardian_role(&e);

    guardian
        .mock_auths(&[MockAuth {
            address: &owner,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "grant_role",
                args: (&monitor, &role).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .grant_role(&monitor, &role);

    // Without the monitor's signature nothing moves.
    assert!(guardian
        .try_set_status(&monitor, &market_id, &FROZEN)
        .is_err());
    assert_eq!(market.get_status(), ACTIVE);

    // The monitor signs the one call to the guardian and nothing deeper. The
    // governance contract accepts the guardian as its owner, and the market
    // accepts the governance contract as its owner, each as its direct caller.
    guardian
        .mock_auths(&[MockAuth {
            address: &monitor,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "set_status",
                args: (&monitor, &market_id, FROZEN).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .set_status(&monitor, &market_id, &FROZEN);
    assert_eq!(market.get_status(), FROZEN);
}

#[test]
fn owner_queues_a_timelocked_call_through_forward() {
    let e = Env::default();
    let owner = Address::generate(&e);
    let guardian_id = e.register(GuardianContract, (&owner,));
    let governance_id = e.register(governance::WASM, (&guardian_id, DELAY));
    let market_id = e.register(MockMarket, (&governance_id,));
    let guardian = GuardianClient::new(&e, &guardian_id);
    let market = MockMarketClient::new(&e, &market_id);
    let timelock = governance::Client::new(&e, &governance_id);

    // forward(governance, "queue", [market, "set_config", [9]]), signed by
    // the owner alone.
    let call_args: Vec<Val> = Vec::from_array(&e, [9u32.into_val(&e)]);
    let queue_args: Vec<Val> = Vec::from_array(
        &e,
        [
            market_id.to_val(),
            Symbol::new(&e, "set_config").to_val(),
            call_args.to_val(),
        ],
    );
    let fn_name = Symbol::new(&e, "queue");
    let nonce: u32 = guardian
        .mock_auths(&[MockAuth {
            address: &owner,
            invoke: &MockAuthInvoke {
                contract: &guardian_id,
                fn_name: "forward",
                args: (&governance_id, &fn_name, &queue_args).into_val(&e),
                sub_invokes: &[],
            },
        }])
        .forward(&governance_id, &fn_name, &queue_args)
        .into_val(&e);

    // Locked until the delay has passed, then anyone executes it.
    assert!(timelock.try_execute(&nonce).is_err());
    assert_eq!(market.get_config(), 0);
    e.ledger().with_mut(|ledger| ledger.timestamp += DELAY);
    timelock.execute(&nonce);
    assert_eq!(market.get_config(), 9);
}
