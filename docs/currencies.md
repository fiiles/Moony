# Currencies and exchange rates

How Moony handles more than one currency. The rules behind it are recorded in
[ADR 0012](./architecture/decisions/0012-explicit-currencies-and-eur-rate-reference.md).

- **Your amounts stay as entered.** Every balance, price, transaction and contract keeps its own
  currency; changing your main currency never rewrites them, it only changes what totals and
  charts are shown in.
- **Main currency.** Any of the 36 currencies with a rate source: the 30 the ECB publishes (AUD,
  BRL, CAD, CHF, CNY, CZK, DKK, EUR, GBP, HKD, HUF, IDR, ILS, INR, ISK, JPY, KRW, MXN, MYR, NOK,
  NZD, PHP, PLN, RON, SEK, SGD, THB, TRY, USD, ZAR) and six with a fixed peg that Moony derives:
  AED, SAR, QAR, BHD and OMR (pegged to the US dollar) and BGN (pegged to the euro). Pegged rates
  follow the dollar or euro rate of each day and are not downloaded. Accounts and holdings can
  also use ARS, CLP, COP, EGP, KWD, RSD, TWD, UAH and VND (45 selectable currencies in all); they
  have no rate source, so they are listed in their own currency and left out of converted totals.
- **Rates.** ECB reference rates (daily rates from the ECB, history via Frankfurter), stored
  against the euro. A historical value is converted at the rate of its own day, or the nearest
  earlier day on which both currencies were quoted (at most ten days back); a gain on a foreign
  stock is computed in your main currency from the cost at each purchase day's rate.
- **Nothing is guessed.** An amount that cannot be converted is never shown 1:1 or at an invented
  rate: it stays visible in its own currency, the totals leave it out, and a note names the
  missing currency. A value whose purchase day had no usable rate and was converted at today's
  rate is marked as estimated. Values in your main currency need no rate at all.
- **Symbols.** Only the main currency is shown with its compact symbol ("$", "€"); every other
  currency is shown with its ISO code.

Where the rates come from, and what is sent to get them, is listed in
[PRIVACY.md](../PRIVACY.md#what-leaves-your-computer-and-when).
