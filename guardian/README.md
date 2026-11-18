# guardian

Owns the Zenex market contracts, directly or through the
[governance timelock](https://github.com/zenex-trade/zenex-contracts) that owns a market. Its owner
forwards calls to them and grants a role that halts a market.

Constructor: `(owner)`.

```
forward(target, fn_name, args) -> Val
set_status(caller, target, status)
grant_role(account, role)
revoke_role(account, role)
```

- `forward` is owner only. It calls `fn_name` on `target` with `args` and returns the result. It
  reaches every owner-only function of the contracts the guardian owns, their ownership transfer
  included.
- `set_status` needs `caller`'s signature and the `guardian` role. It moves the market `target` to a
  stricter status: from Active (0) to OnIce (1) or Frozen (2), or from OnIce to Frozen. It reads
  the market's owner. A market the guardian owns gets the status directly. Any other owner is
  assumed to be the governance contract and gets the call.
- `grant_role` and `revoke_role` are owner only. `guardian` is the only role.
- After `renounce_ownership` every function above fails, `set_status` included.

Resuming, delisting and retiring a market stay with the owner, through `forward`.

Ownership is OpenZeppelin's `Ownable`: `get_owner`, `transfer_ownership`, `accept_ownership` and
`renounce_ownership`.

## Events

| | |
|---|---|
| `status_set` | topics `["status_set", target: Address]`, data `{status: u32, caller: Address}` |
| `forwarded` | topics `["forwarded", target: Address]`, data `{fn_name: Symbol}` |

Role and ownership changes emit OpenZeppelin's events.

| Error | |
|---|---|
| 1 | `caller` does not hold the `guardian` role |
| 820 | The status is not operational, or not stricter than the current one |
| 821 | The market is Delisted or Retired |
| 822 | The role is not `guardian` |
| 823 | The guardian has no owner |
