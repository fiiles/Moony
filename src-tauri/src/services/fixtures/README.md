# Test fixtures of `services/`

## `yahoo_chart_aapl_split.json`

A Yahoo Finance `v8/finance/chart` response for AAPL, 2020-08-28 to 2020-09-02, daily
interval, with the `events.splits` block of the 4:1 split effective 2020-08-31. Read by the
`chart_splits_*` and `series_carries_prices_and_splits` tests in `price_api.rs`.

It is **not** a captured response: Yahoo answered `Too Many Requests` to every capture attempt
from the development machine on 2026-10-24. It was built by hand from the documented shape of
the chart endpoint (the same fields `yahoo_finance_api` 5 deserialises), with the split event
written exactly as Yahoo reports it (spec D2):

```json
"splits": { "1598880600": { "date": 1598880600, "numerator": 4, "denominator": 1, "splitRatio": "4:1" } }
```

`numerator 4 / denominator 1` means one old share became four. The `yahoo_finance_api` doc
comment on `Split` describes the fields the other way round and is wrong. The closes are the
split-adjusted daily closes of those four sessions, trimmed to keep the file small. Replace it
with a real capture when one is at hand (keep the same days and trim the arrays).
