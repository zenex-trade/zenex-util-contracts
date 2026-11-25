# wallet-factory

Deploys the current smart-account wasm and emits an event for each wallet.

Constructor: `(owner, account_wasm_hash)`.

```
deploy(signer) -> Address
predict_address(signer) -> Address
set_wasm(account_wasm_hash)        // owner only
```

`deploy` creates the wallet of one signer, with no policies. The wallet's owner adds signers and
policies on the wallet afterwards. The salt is the SHA-256 of the signer as XDR, so a signer has
one wallet under this factory and anyone may deploy it. Deploying the same signer twice fails. The
address does not depend on the wasm.

## Owner

The owner sets the wasm that new wallets are deployed with.

The owner has no power over a wallet that exists. A wallet keeps the wasm it was deployed with
until it upgrades itself with its own signers' authorization, so read a wallet's current code from
the wallet. The factory's instance storage carries the wasm it currently deploys, and each
`wallet_deployed` event the wasm of that wallet.

Ownership is OpenZeppelin's `Ownable`: a two-step transfer, and `renounce_ownership`, after which
the wasm can never change.

## Events

Each successful `deploy` emits one `wallet_deployed` event.

| | |
|---|---|
| topics | `["wallet_deployed", wallet: Address]` |
| data | `{account_wasm_hash: BytesN<32>}` |

The event carries no signer or caller. The wallet stores its signers, and
`deploy` requires no authorization, so the caller says nothing about the wallet's owner.

`set_wasm` emits `wasm_set` with the data `{account_wasm_hash: BytesN<32>}`.
Ownership changes emit OpenZeppelin's events.

## Counting deployments

The factory's `wallet_deployed` events are the list of its wallets: one event is one wallet, and a
failed `deploy` emits nothing. The host emits no event of its own when a contract is created, so without this event
a counter would have to read the metadata of every transaction that reaches the factory.

Ask Stellar RPC for the factory's contract events and page with the returned cursor until a page is
empty:

```
curl -s <rpc_url> -H 'Content-Type: application/json' -d '{
  "jsonrpc": "2.0", "id": 1, "method": "getEvents",
  "params": {
    "startLedger": <ledger>,
    "filters": [{
      "type": "contract",
      "contractIds": ["<factory_id>"],
      "topics": [["AAAADwAAAA93YWxsZXRfZGVwbG95ZWQA", "*"]]
    }],
    "pagination": {"limit": 1000}
  }
}'
```

The first topic is the base64 XDR of the symbol `wallet_deployed`. The second topic of each event is
the wallet, and the event's ledger is the ledger it was created in. To ask whether one address is a
wallet of this factory, put the address's XDR in place of the `*`.

Stellar RPC serves events from a limited window of recent ledgers, seven days by default. A count
over the factory's whole life needs an indexer that starts at the factory's deployment ledger and
keeps what it reads.
