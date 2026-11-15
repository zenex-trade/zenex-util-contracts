# Zenex Util Contracts

Soroban contracts used alongside the [Zenex](https://docs.zenex.trade) perpetual futures markets on
[Stellar](https://stellar.org). They are separate from the
[core contracts](https://github.com/zenex-trade/zenex-contracts): none of them holds user funds
between calls, and none has an admin or an upgrade function.

## Contracts

| Crate | Description |
|-------|-------------|
| [`fee-forwarder`](fee-forwarder/README.md) | Lets a user pay a relayer in a token instead of paying the network fee. It collects the fee the user signed a cap for, sends it to the fee recipient and makes the user's call in the same invocation. A failing call reverts the fee, and the contract keeps no state. |
| [`market-router`](market-router/README.md) | Batches contract calls into one transaction, all-or-nothing or with a result per call. It also runs the create-and-fill flows, which create an order on a market and fill it in the same transaction. It is stateless and holds nothing. |
| [`session-policy`](session-policy/README.md) | A policy for an OpenZeppelin smart account that limits a session key to trading. The key may call the configured markets, move the collateral token into them, and pay relay fees through the forwarder to one fixed recipient. Everything else is refused, and the configuration is fixed at deployment. |
| [`referral`](referral/README.md) | Records that one wallet was referred by another. The referred wallet authorizes the call and the contract emits an event, which is the whole record. There is no storage. |
| [`wallet-factory`](wallet-factory/README.md) | Deploys smart-account wallets from one WASM fixed at deployment. The address is derived from the wallet's signers and policies, so anyone can deploy a wallet and nobody can deploy a different one at its address. |

## Getting Started

Install Rust through [rustup](https://rustup.rs/) and the
[Stellar CLI 27.0.0 release binary](https://github.com/stellar/stellar-cli/releases/tag/v27.0.0)
on `PATH`.

Run commands from the repository root. Rustup installs Rust 1.98.1 and the `wasm32v1-none` target
from `rust-toolchain.toml`. Make targets use the committed lockfile.

```
make        # build the optimized WASMs into target/wasm32v1-none/release/
make test
make fmt
```

The tests need no built WASM. The WASMs they load are checked in:

- [market-router/testdata/market.wasm](market-router/testdata/market.wasm) is a build of the Zenex
  `market` contract from the [core contracts](https://github.com/zenex-trade/zenex-contracts)
  (AGPL-3.0-only).
- [session-policy/testdata/smart_account.wasm](session-policy/testdata/smart_account.wasm) and
  [session-policy/testdata/ed25519_verifier.wasm](session-policy/testdata/ed25519_verifier.wasm) are
  the smart account and its ed25519 verifier from
  [smart-account-kit](https://github.com/kalepail/smart-account-kit)
  ([Apache-2.0](https://github.com/kalepail/smart-account-kit/blob/main/LICENSE)), which builds on
  OpenZeppelin's [stellar-contracts](https://github.com/OpenZeppelin/stellar-contracts)
  ([MIT](https://github.com/OpenZeppelin/stellar-contracts/blob/main/LICENSE)).

## Dependencies

`fee-forwarder`, `session-policy` and `wallet-factory` use OpenZeppelin's
[stellar-contracts](https://github.com/OpenZeppelin/stellar-contracts) crates
`stellar-fee-abstraction` and `stellar-accounts`. They are taken from git at commit
[`df602b613fbc4ae1e98ff62ba3caef671a370f65`](https://github.com/OpenZeppelin/stellar-contracts/commit/df602b613fbc4ae1e98ff62ba3caef671a370f65),
not from crates.io. That commit is not a tagged release, and OpenZeppelin has not published it as an
audited version. `Cargo.toml` states why this revision is used and `Cargo.lock` pins it.

## Releases

Release WASMs are built by the [release workflow](.github/workflows/release.yml) when a version tag is
pushed. It builds each contract with stellar.expert's
[soroban-build-workflow](https://github.com/stellar-expert/soroban-build-workflow) and Stellar CLI
27.0.0. For each contract it publishes a GitHub release with the WASM, attests the build provenance,
and submits the hash to [stellar.expert](https://stellar.expert) for source validation. A contract's
release is named `<tag>_<package>_cli27.0.0` and its file `<package>_<tag>.wasm`.

Deploy the release files unchanged. Do not deploy a local build: only the release files are attested
and validated. The network stores contract code under the SHA-256 of its bytes, so a deployed
contract's WASM hash equals the SHA-256 of its release file.

Each release WASM embeds the `source_repo` and `home_domain` contract meta. The build is reproducible
with the pinned Rust toolchain and Stellar CLI: at a release tag, `make release` writes the same
files to `wasm/`. It needs a clean checkout.

## Checking a deployed contract

Fetch the contract's WASM, find the release whose file has the same hash, and rebuild that release's
tag:

```
stellar contract fetch --id <contract_id> --network <network> --out-file deployed.wasm
stellar contract info meta --wasm deployed.wasm
git checkout <release tag>
make release
sha256sum deployed.wasm wasm/*.wasm
```

The meta names the repository the WASM was built in. `deployed.wasm` must have the hash of one of
the files in `wasm/` and of that contract's release file.

`gh attestation verify <file>.wasm --repo zenex-trade/zenex-util-contracts` checks the provenance of
a downloaded release file.

## Security

See [SECURITY.md](SECURITY.md) for how to report a vulnerability.

## License

[AGPL-3.0-only](LICENSE)
