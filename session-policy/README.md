# session-policy

OpenZeppelin smart-account policy for a trading session key.

Constructor: `(forwarder, markets, token, fee_recipient)`.

Install takes `limit`: the most the session key may spend under the rule, in the token's units.
The wallet sets it, and it is stored per wallet and rule. The count never resets while the policy
stays installed: a new limit is a new session rule.

A context is allowed only when the session key signed and it is:

- a forwarder call whose signed `fee_token` (argument 0) is `token` and whose signed
  `fee_recipient` (argument 3) is `fee_recipient`;
- a `transfer` of `token` into a market, or an `approve` of `token` to the forwarder;
- any call to a market.

Everything else is refused.

## Spend limit

Two things count as spent:

- the amount of each `transfer` of `token` into a market;
- the amount of each `approve` of `token` to the forwarder. The forwarder approves the signed
  `max_fee_amount` on every call, so that counts, whatever fee the relayer sets.

A context that would take the rule's spending over `limit` is refused. A market call that moves no
token spends nothing.

The count is gross: collateral a market pays back to the wallet does not lower it. An amount counts
when the wallet authorizes it. A call that fails reverts its count with it, unless a contract in
between catches the failure.

The limit and the amount spent are the persistent entry `Spend(smart_account, context_rule_id)`,
a `{limit, spent}`. There is no getter: read the entry from the ledger.

| Event | Topics | Data |
|---|---|---|
| `session_installed` | `smart_account` | `context_rule_id`, `limit` |
| `session_spent` | `smart_account` | `context_rule_id`, `amount`, `total_spent` |
| `session_uninstalled` | `smart_account` | `context_rule_id` |

| Error | |
|---|---|
| 4002 | Contract not allowed |
| 4003 | Token function not allowed |
| 4004 | Transfer destination is not a market |
| 4005 | Approve spender is not the forwarder |
| 4006 | Wrong fee token or fee recipient |
| 4007 | Session key did not sign |
| 4008 | Spend limit exceeded |
| 4009 | Transfer or approve amount missing or negative |
| 4010 | Negative limit at install |
| 4011 | No limit stored for the wallet and rule |
| 4012 | Limit already stored for the wallet and rule |
