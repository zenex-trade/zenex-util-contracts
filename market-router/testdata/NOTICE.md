# Test WASM in this directory

`market.wasm` (SHA-256 `02aa342f5e6e084c8e5728b74aee77864f77ea720896e61a689da8252f95943c`, 80,038
bytes) is a build of the Zenex `market` contract from the
[core contracts](https://github.com/zenex-trade/zenex-contracts), under
[AGPL-3.0-only](https://github.com/zenex-trade/zenex-contracts/blob/main/LICENSE), the licence of
this repository. The `market-router` tests import its interface and run it. It is not part of a
release of this repository.

It was built with Rust 1.98.1, soroban-sdk 26.1.1 and Stellar CLI 27.1.0 from a development commit
that precedes the core contracts' public history, so it is not one of their release files. Its
contract meta carries the build details:

```
stellar contract info meta --wasm market.wasm
```
