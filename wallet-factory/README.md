# wallet-factory

Deploys the pinned smart-account wasm and emits an event for each wallet.

Constructor: `(account_wasm_hash)`.

```
deploy(signers, policies) -> Address
predict_address(signers, policies) -> Address
account_wasm_hash() -> BytesN<32>
```

The salt is the SHA-256 of `(signers, policies)` as XDR, so the address commits to the wallet's
signers and anyone may deploy it. Deploying the same arguments twice fails.

## No admin

The factory has no owner, no setter and no upgrade function. `account_wasm_hash` returns the hash
passed to the constructor, and every wallet of the factory runs that build. A new account build
needs a new factory, at a new address. A wallet's address depends on the factory's address, so the
same signers get a different address from a different factory.

## Events

Each successful `deploy` emits one `wallet_deployed` event. The factory emits no other event.

| | |
|---|---|
| topics | `["wallet_deployed", wallet: Address]` |
| data | `{account_wasm_hash: BytesN<32>}` |

The event carries no signers, policies or caller. The wallet stores its signers and policies, and
`deploy` requires no authorization, so the caller says nothing about the wallet's owner.

## Counting deployments

The factory's events are the list of its wallets: one event is one wallet, and a failed `deploy`
emits nothing. The host emits no event of its own when a contract is created, so without this event
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
