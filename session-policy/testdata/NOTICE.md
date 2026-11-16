# Test WASMs in this directory

Both files are third-party contracts, used unchanged by the tests of `session-policy` and
`wallet-factory`. Neither is part of a release of this repository.

| File | SHA-256 | Size |
|------|---------|-----:|
| `smart_account.wasm` | `c514d35ea67eaea9bfc1fb3327854c0c06111bc1ddebd4fe217c504c1004b9e1` | 48,273 |
| `ed25519_verifier.wasm` | `60e8798db610bdaf3370d39ebda56ee1dc2c15ce1c3a9e28b528bfa24a06b477` | 1,848 |

## smart_account.wasm

- Source: OpenZeppelin [stellar-contracts](https://github.com/OpenZeppelin/stellar-contracts),
  package `multisig-account-example` (`examples/multisig-smart-account/account`), at tag `v0.8.0`,
  commit `ddc327eb92d63709436da3a8455f641c2d5476c5`. It is built on `stellar-accounts` 0.8.0, which
  carries the fix of advisory GHSA-jpg9-m7w4-c7wc.
- Build: by the [smart-account-kit](https://github.com/stellar/smart-account-kit) project, with
  Rust 1.91.1, soroban-sdk 28.0.0 and Stellar CLI 28.1.0, as recorded in its
  [deployment record](https://github.com/stellar/smart-account-kit/blob/0efdd609072086999553d16ee6591ce9b803c470/docs/deployments-stellar-accounts-0.8-2026-10-25.md).
  The kit publishes the hash, not the file. The file here was fetched from the network by that
  hash, and testnet and mainnet returned the same bytes:

  ```
  stellar contract fetch --wasm-hash c514d35ea67eaea9bfc1fb3327854c0c06111bc1ddebd4fe217c504c1004b9e1 \
    --network <network> --out-file smart_account.wasm
  ```

## ed25519_verifier.wasm

- Source: OpenZeppelin [stellar-contracts](https://github.com/OpenZeppelin/stellar-contracts),
  package `multisig-ed25519-verifier-example` (`examples/multisig-smart-account/ed25519-verifier`),
  at commit `1e513890ecf79833c9d6e7ef38a9358001c0b111`.
- Build: by the smart-account-kit project, with Rust 1.91.1, soroban-sdk 26.1.0 and Stellar CLI
  27.0.0, as recorded in its
  [deployment record](https://github.com/stellar/smart-account-kit/blob/fc99beabc581c87c1100547a604e12db7a4e652d/docs/deployments-protocol-27-2026-07-09.md).
  It is the WASM of the verifier contracts that record lists, which wallets on the v0.8.0 account
  keep using.

## Licences

OpenZeppelin stellar-contracts is under the MIT licence, reproduced below. smart-account-kit is
under the [Apache License 2.0](https://github.com/stellar/smart-account-kit/blob/main/LICENSE).

```
MIT License

Copyright (c) 2024 OpenZeppelin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
