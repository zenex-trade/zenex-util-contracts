# Test WASM in this directory

`governance_v1.0.0.wasm` (SHA-256
`a9bcea326b12b99ac09a3ce79f54bb4ebd9391b442206e99fec488b4097104d3`, 11,491 bytes) is the Zenex
`governance` contract from the
[core contracts](https://github.com/zenex-trade/zenex-contracts), under
[AGPL-3.0-only](https://github.com/zenex-trade/zenex-contracts/blob/main/LICENSE), the licence of
this repository. It is the release asset of
[`v1.0.0_governance_cli27.0.0`](https://github.com/zenex-trade/zenex-contracts/releases/tag/v1.0.0_governance_cli27.0.0),
unchanged. The guardian tests run it to check `set_status` and `forward` against the real
timelock. It is not part of a release of this repository.
