# session-policy

OpenZeppelin smart-account policy for a trading session key.

Constructor: `(forwarder, markets, token, fee_recipient)`.

Install takes `{limit, period_ledgers}`: the most the session key may spend per period, in the
token's units, and the length of a period in ledgers. The wallet sets it, and it is stored per
wallet and rule. A new limit is a new session rule.

A context is allowed only when the session key signed and it is:

- a forwarder call whose signed `fee_recipient` (argument 3) is `fee_recipient`;
- a `transfer` of `token` into a market, or an `approve` of `token` to the forwarder;
- any call to a market.

Everything else is refused.

## Spend limit

Two things count as spent:

- the amount of each `transfer` of `token` into a market;
- the signed `max_fee_amount` (argument 1) of each forwarder call, whatever fee the relayer sets.

A period starts at the first spend and ends `period_ledgers` later. The next spend after that
starts a new period at zero. A context that would take the period's spending over `limit` is
refused. An `approve` and a market call that moves no token spend nothing.

The count is gross: collateral a market pays back to the wallet does not lower it.

```
get_spend(smart_account, context_rule_id) -> Option<{limit, period_ledgers, period_start, spent}>
```

`spent` is the amount since `period_start`. Once the ledger is `period_start + period_ledgers` or
later, the period is over and the amount spent is zero, although `spent` keeps its last value until
the next spend.

| Error | |
|---|---|
| 4002 | Contract not allowed |
| 4003 | Token function not allowed |
| 4004 | Transfer destination is not a market |
| 4005 | Approve spender is not the forwarder |
| 4006 | Wrong fee recipient |
| 4007 | Session key did not sign |
| 4008 | Spend limit exceeded |
| 4009 | Transfer amount or maximum fee missing or negative |
| 4010 | Negative limit or zero period at install |
| 4011 | No limit stored for the wallet and rule |
