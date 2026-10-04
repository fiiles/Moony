//! Yahoo Finance lookups for the instruments of a file.
//!
//! Network only: never called with the database lock held. Lookups run one
//! after another, [`LOOKUP_DELAY`] apart, at most [`MAX_QUERIES`] instruments
//! per call; a lookup that fails marks that instrument unverified
//! (`lookup_failed`), never the whole call, and a run of failures (offline, rate
//! limited) stops the requests so the wizard never waits on a dead network.
//!
//! The symbol is searched first and the ISIN when the symbol found nothing (or
//! there is none). The listings found come with the currency their exchange
//! suffix implies, and `best` is the one the import should store the trades
//! under; see [`choose_best`]. The chosen listing then costs one more request, its
//! quote, which names the currency it really trades in (see [`listing_currency`]);
//! the other listings keep the guess. Every request, searches and quotes alike, is
//! paced by [`LOOKUP_DELAY`].

use std::future::Future;
use std::time::Duration;

use super::types::{StockInstrumentCandidate, StockInstrumentQuery, StockInstrumentResolution};
use crate::error::Result;
use crate::services::price_api::{self, StockSearchResult};
use crate::services::quote_unit::{is_pence, quote_unit};

/// Pause between two Yahoo Finance requests.
const LOOKUP_DELAY: Duration = Duration::from_millis(250);
/// Instruments looked up per call; the rest come back unverified.
pub const MAX_QUERIES: usize = 50;
/// Listings returned per instrument.
const MAX_CANDIDATES: usize = 10;
/// Failed instruments in a row after which no more requests are made.
const MAX_CONSECUTIVE_FAILURES: usize = 3;

/// Look every query up (symbol first, ISIN otherwise); failures are reported
/// per instrument (`lookupFailed`), never as an error of the whole call.
/// Instruments past [`MAX_QUERIES`], or after a run of failures, are reported
/// as failed without a request: unverified, to be asked again.
pub async fn resolve_instruments(
    queries: Vec<StockInstrumentQuery>,
) -> Vec<StockInstrumentResolution> {
    resolve_with(
        queries,
        LOOKUP_DELAY,
        |term: String| async move { price_api::search_stock_tickers(&term).await },
        |symbol: String| async move { price_api::get_quote_currency(&symbol).await },
    )
    .await
}

fn unverified(key: String) -> StockInstrumentResolution {
    StockInstrumentResolution {
        key,
        candidates: Vec::new(),
        best: None,
        lookup_failed: true,
    }
}

/// What to search for, in order: the symbol, then the ISIN.
fn search_terms(query: &StockInstrumentQuery) -> Vec<String> {
    let clean = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let mut terms: Vec<String> = clean(&query.symbol).into_iter().collect();
    if let Some(isin) = clean(&query.isin).map(|isin| isin.to_uppercase()) {
        if !terms.iter().any(|t| t.eq_ignore_ascii_case(&isin)) {
            terms.push(isin);
        }
    }
    terms
}

/// The listings Yahoo found: duplicates (by symbol) dropped, the currency of
/// each from its exchange suffix, at most [`MAX_CANDIDATES`].
fn candidates_of(found: Vec<StockSearchResult>) -> Vec<StockInstrumentCandidate> {
    let mut candidates: Vec<StockInstrumentCandidate> = Vec::new();
    for result in found {
        let symbol = result.symbol.trim().to_string();
        if symbol.is_empty()
            || candidates
                .iter()
                .any(|c| c.symbol.eq_ignore_ascii_case(&symbol))
        {
            continue;
        }
        candidates.push(StockInstrumentCandidate {
            currency: price_api::get_currency_from_ticker(&symbol).to_string(),
            name: result.shortname,
            exchange: result.exchange,
            symbol,
        });
        if candidates.len() == MAX_CANDIDATES {
            break;
        }
    }
    candidates
}

/// The currency a chosen listing is shown, and its trades stored, in: what Yahoo reports for its
/// quote. Pence (`GBp`, `GBX`) are `GBX`, the code that tells the simulation the file's prices are
/// in pence; every other unit is the currency itself, and without a reported code the suffix
/// guess stands.
fn listing_currency(reported: Option<&str>, symbol: &str) -> String {
    if reported.map(str::trim).is_some_and(is_pence) {
        return "GBX".to_string();
    }
    quote_unit(reported, symbol).currency
}

/// The listing the trades should be stored under.
///
/// In order: the exact symbol (case-insensitive) in the trade currency; any
/// listing in the trade currency; the exact symbol; the first listing. Without
/// a trade currency that is simply the exact symbol, else the first.
///
/// The exact symbol only wins outright when its currency does not contradict
/// the trades: a symbol such as `VOD` or `SAP` exists as a US listing too, and
/// storing euro or pound trades under that would value the position in the
/// wrong currency.
pub(crate) fn choose_best(
    symbol: Option<&str>,
    currency: Option<&str>,
    candidates: &[StockInstrumentCandidate],
) -> Option<StockInstrumentCandidate> {
    let symbol = symbol.map(str::trim).filter(|s| !s.is_empty());
    let currency = currency.map(str::trim).filter(|c| !c.is_empty());
    let exact =
        |c: &&StockInstrumentCandidate| symbol.is_some_and(|s| c.symbol.eq_ignore_ascii_case(s));
    let in_currency = |c: &&StockInstrumentCandidate| {
        currency.is_some_and(|cur| c.currency.eq_ignore_ascii_case(cur))
    };
    candidates
        .iter()
        .find(|c| exact(c) && (currency.is_none() || in_currency(c)))
        .or_else(|| candidates.iter().find(in_currency))
        .or_else(|| candidates.iter().find(exact))
        .or_else(|| candidates.first())
        .cloned()
}

/// [`resolve_instruments`] with the search and the quote lookup injected (tests, and the delay).
async fn resolve_with<F, Fut, Q, QFut>(
    queries: Vec<StockInstrumentQuery>,
    delay: Duration,
    mut search: F,
    mut quote: Q,
) -> Vec<StockInstrumentResolution>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<StockSearchResult>>>,
    Q: FnMut(String) -> QFut,
    QFut: Future<Output = Result<Option<String>>>,
{
    let mut resolutions = Vec::with_capacity(queries.len());
    let mut looked_up = 0usize;
    let mut requests = 0usize;
    let mut failures_in_a_row = 0usize;
    let mut quote_failures_in_a_row = 0usize;

    for query in queries {
        if looked_up >= MAX_QUERIES || failures_in_a_row >= MAX_CONSECUTIVE_FAILURES {
            resolutions.push(unverified(query.key));
            continue;
        }
        let terms = search_terms(&query);
        if terms.is_empty() {
            // Nothing to ask Yahoo: unknown, not a failure.
            resolutions.push(StockInstrumentResolution {
                key: query.key,
                candidates: Vec::new(),
                best: None,
                lookup_failed: false,
            });
            continue;
        }
        looked_up += 1;

        let mut candidates = Vec::new();
        let mut failed = false;
        for term in terms {
            if requests > 0 {
                tokio::time::sleep(delay).await;
            }
            requests += 1;
            match search(term).await {
                Ok(found) => {
                    candidates = candidates_of(found);
                    if !candidates.is_empty() {
                        break;
                    }
                }
                Err(e) => {
                    log::debug!("[STOCK IMPORT] Instrument lookup failed: {e}");
                    failed = true;
                    break;
                }
            }
        }
        failures_in_a_row = if failed { failures_in_a_row + 1 } else { 0 };

        let mut best = if failed {
            None
        } else {
            choose_best(
                query.symbol.as_deref(),
                query.currency.as_deref(),
                &candidates,
            )
        };

        // One more request, for the listing that was chosen: its quote names the currency it
        // really trades in. A listing's exchange suffix only guesses it (VUSD.L is in dollars,
        // not pounds), and it is the guess the ranking above went by: asking every candidate
        // would multiply the requests. A failed lookup keeps the guess and is no failure of the
        // instrument; a run of them stops the lookups, as a run of failed searches stops all.
        if let Some(chosen) = best.as_mut() {
            if quote_failures_in_a_row < MAX_CONSECUTIVE_FAILURES {
                if requests > 0 {
                    tokio::time::sleep(delay).await;
                }
                requests += 1;
                match quote(chosen.symbol.clone()).await {
                    Ok(reported) => {
                        quote_failures_in_a_row = 0;
                        let currency = listing_currency(reported.as_deref(), &chosen.symbol);
                        if let Some(listed) =
                            candidates.iter_mut().find(|c| c.symbol == chosen.symbol)
                        {
                            listed.currency = currency.clone();
                        }
                        chosen.currency = currency;
                    }
                    Err(e) => {
                        log::debug!("[STOCK IMPORT] Quote currency lookup failed: {e}");
                        quote_failures_in_a_row += 1;
                    }
                }
            }
        }

        resolutions.push(StockInstrumentResolution {
            key: query.key,
            candidates: if failed { Vec::new() } else { candidates },
            best,
            lookup_failed: failed,
        });
    }
    resolutions
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::error::AppError;

    /// What a quote lookup that has nothing to say answers.
    async fn no_quote(_symbol: String) -> Result<Option<String>> {
        Ok(None)
    }

    /// [`resolve_with`] for the tests of the search: no quote is looked up.
    async fn resolve_without_quotes<F, Fut>(
        queries: Vec<StockInstrumentQuery>,
        delay: Duration,
        search: F,
    ) -> Vec<StockInstrumentResolution>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<Vec<StockSearchResult>>>,
    {
        resolve_with(queries, delay, search, no_quote).await
    }

    fn candidate(symbol: &str, currency: &str) -> StockInstrumentCandidate {
        StockInstrumentCandidate {
            symbol: symbol.to_string(),
            name: format!("{symbol} name"),
            exchange: "EXC".to_string(),
            currency: currency.to_string(),
        }
    }

    fn best(
        symbol: Option<&str>,
        currency: Option<&str>,
        candidates: &[StockInstrumentCandidate],
    ) -> Option<String> {
        choose_best(symbol, currency, candidates).map(|c| c.symbol)
    }

    // ---- the pure candidate choice -------------------------------------------

    #[test]
    fn the_exact_symbol_wins_whatever_its_position_and_case() {
        let listings = [
            candidate("VUSA.L", "GBP"),
            candidate("AAPL", "USD"),
            candidate("AAPL.MX", "USD"),
        ];
        assert_eq!(best(Some("aapl"), None, &listings).as_deref(), Some("AAPL"));
        assert_eq!(
            best(Some("AAPL"), Some("USD"), &listings).as_deref(),
            Some("AAPL")
        );
        assert_eq!(
            best(Some(" AAPL "), Some("usd"), &listings).as_deref(),
            Some("AAPL")
        );
    }

    #[test]
    fn without_an_exact_symbol_the_first_listing_in_the_trade_currency_wins() {
        let listings = [
            candidate("VUSA.L", "GBP"),
            candidate("VUSA.AS", "EUR"),
            candidate("VUSA.DE", "EUR"),
        ];
        assert_eq!(
            best(Some("VUSA"), Some("EUR"), &listings).as_deref(),
            Some("VUSA.AS")
        );
        assert_eq!(
            best(Some("VUSA"), Some("gbp"), &listings).as_deref(),
            Some("VUSA.L")
        );
    }

    #[test]
    fn with_nothing_to_prefer_the_first_listing_wins() {
        let listings = [candidate("VUSA.L", "GBP"), candidate("VUSA.AS", "EUR")];
        assert_eq!(
            best(Some("VUSA"), None, &listings).as_deref(),
            Some("VUSA.L")
        );
        assert_eq!(
            best(Some("VUSA"), Some("CHF"), &listings).as_deref(),
            Some("VUSA.L")
        );
        assert_eq!(best(None, None, &listings).as_deref(), Some("VUSA.L"));
    }

    #[test]
    fn an_isin_search_has_no_symbol_and_prefers_the_trade_currency() {
        let listings = [candidate("VWRL.L", "GBP"), candidate("VWRL.AS", "EUR")];
        assert_eq!(
            best(None, Some("EUR"), &listings).as_deref(),
            Some("VWRL.AS")
        );
    }

    #[test]
    fn an_exact_symbol_in_another_currency_loses_to_a_listing_in_the_trade_currency() {
        // `VOD` is Vodafone's US listing; the trades are in pounds.
        let listings = [candidate("VOD", "USD"), candidate("VOD.L", "GBP")];
        assert_eq!(
            best(Some("VOD"), Some("GBP"), &listings).as_deref(),
            Some("VOD.L")
        );
        // Without a listing in the trade currency the exact symbol is still the best guess.
        assert_eq!(
            best(Some("VOD"), Some("EUR"), &listings).as_deref(),
            Some("VOD")
        );
        // And with no currency to contradict, it wins as documented.
        assert_eq!(best(Some("VOD"), None, &listings).as_deref(), Some("VOD"));
    }

    #[test]
    fn no_listings_no_best() {
        assert_eq!(best(Some("AAPL"), Some("USD"), &[]), None);
    }

    // ---- candidates ------------------------------------------------------------

    fn found(symbol: &str) -> StockSearchResult {
        StockSearchResult {
            symbol: symbol.to_string(),
            shortname: format!("{symbol} Inc"),
            exchange: "NMS".to_string(),
        }
    }

    #[test]
    fn candidates_carry_the_currency_of_their_exchange_suffix() {
        let candidates = candidates_of(vec![
            found("AAPL"),
            found("VUSA.L"),
            found("SPYL.DE"),
            found("NOVN.SW"),
        ]);
        let pairs: Vec<(&str, &str)> = candidates
            .iter()
            .map(|c| (c.symbol.as_str(), c.currency.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("AAPL", "USD"),
                ("VUSA.L", "GBP"),
                ("SPYL.DE", "EUR"),
                ("NOVN.SW", "CHF")
            ]
        );
        assert_eq!(candidates[0].name, "AAPL Inc");
        assert_eq!(candidates[0].exchange, "NMS");
    }

    #[test]
    fn duplicates_and_blanks_are_dropped_and_the_list_is_capped() {
        let mut results = vec![found("AAPL"), found("aapl"), found("  "), found("AAPL")];
        results.extend((0..15).map(|i| found(&format!("S{i}"))));
        let candidates = candidates_of(results);
        assert_eq!(candidates.len(), MAX_CANDIDATES);
        assert_eq!(candidates[0].symbol, "AAPL");
        assert_eq!(candidates[1].symbol, "S0");
    }

    // ---- the lookups -----------------------------------------------------------

    fn query(
        key: &str,
        symbol: Option<&str>,
        isin: Option<&str>,
        currency: Option<&str>,
    ) -> StockInstrumentQuery {
        StockInstrumentQuery {
            key: key.to_string(),
            symbol: symbol.map(str::to_string),
            isin: isin.map(str::to_string),
            name: None,
            currency: currency.map(str::to_string),
        }
    }

    const NO_DELAY: Duration = Duration::ZERO;

    #[tokio::test]
    async fn the_symbol_is_looked_up_and_the_best_listing_chosen() {
        let mut asked: Vec<String> = Vec::new();
        let resolutions = resolve_without_quotes(
            vec![query(
                "symbol:VUSA",
                Some("VUSA"),
                Some("IE00B3XXRP09"),
                Some("EUR"),
            )],
            NO_DELAY,
            |term| {
                asked.push(term);
                async { Ok(vec![found("VUSA.L"), found("VUSA.AS")]) }
            },
        )
        .await;

        assert_eq!(asked, vec!["VUSA"], "the ISIN is not needed");
        let r = &resolutions[0];
        assert_eq!(r.key, "symbol:VUSA");
        assert!(!r.lookup_failed);
        assert_eq!(r.candidates.len(), 2);
        assert_eq!(r.best.as_ref().map(|c| c.symbol.as_str()), Some("VUSA.AS"));
    }

    #[tokio::test]
    async fn the_isin_is_searched_when_there_is_no_symbol_or_it_found_nothing() {
        let mut asked: Vec<String> = Vec::new();
        let resolutions = resolve_without_quotes(
            vec![
                query("isin:A", None, Some("ie00b3rbwm25"), None),
                query("symbol:ZZZ", Some("ZZZ"), Some("US0378331005"), None),
            ],
            NO_DELAY,
            |term| {
                asked.push(term.clone());
                async move {
                    Ok(if term == "ZZZ" {
                        vec![]
                    } else {
                        vec![found("HIT")]
                    })
                }
            },
        )
        .await;

        assert_eq!(asked, vec!["IE00B3RBWM25", "ZZZ", "US0378331005"]);
        assert_eq!(
            resolutions[0].best.as_ref().map(|c| c.symbol.as_str()),
            Some("HIT")
        );
        assert_eq!(
            resolutions[1].best.as_ref().map(|c| c.symbol.as_str()),
            Some("HIT")
        );
    }

    #[tokio::test]
    async fn an_instrument_nothing_was_found_for_is_unknown_not_failed() {
        let resolutions = resolve_without_quotes(
            vec![query("symbol:ZZZ", Some("ZZZ"), None, None)],
            NO_DELAY,
            |_| async { Ok(vec![]) },
        )
        .await;
        let r = &resolutions[0];
        assert!(r.candidates.is_empty() && r.best.is_none());
        assert!(!r.lookup_failed);
    }

    #[tokio::test]
    async fn a_query_with_neither_symbol_nor_isin_makes_no_request() {
        let mut requests = 0;
        let resolutions = resolve_without_quotes(
            vec![
                query("x", Some("  "), None, None),
                query("y", None, Some(""), None),
            ],
            NO_DELAY,
            |_| {
                requests += 1;
                async { Ok(vec![found("X")]) }
            },
        )
        .await;
        assert_eq!(requests, 0);
        assert!(resolutions
            .iter()
            .all(|r| !r.lookup_failed && r.best.is_none()));
    }

    #[tokio::test]
    async fn a_failed_lookup_marks_only_that_instrument() {
        let resolutions = resolve_without_quotes(
            vec![
                query("a", Some("AAA"), None, None),
                query("b", Some("BBB"), None, None),
                query("c", Some("CCC"), None, None),
            ],
            NO_DELAY,
            |term| async move {
                if term == "BBB" {
                    Err(AppError::ExternalApi("offline".into()))
                } else {
                    Ok(vec![found(&term)])
                }
            },
        )
        .await;

        let failed: Vec<bool> = resolutions.iter().map(|r| r.lookup_failed).collect();
        assert_eq!(failed, vec![false, true, false]);
        assert!(resolutions[1].candidates.is_empty() && resolutions[1].best.is_none());
        assert_eq!(
            resolutions[2].best.as_ref().map(|c| c.symbol.as_str()),
            Some("CCC")
        );
    }

    #[tokio::test]
    async fn after_a_run_of_failures_no_more_requests_are_made() {
        let mut requests = 0;
        let queries: Vec<StockInstrumentQuery> = (0..8)
            .map(|i| query(&format!("k{i}"), Some(&format!("S{i}")), None, None))
            .collect();
        let resolutions = resolve_without_quotes(queries, NO_DELAY, |_| {
            requests += 1;
            async { Err(AppError::ExternalApi("offline".into())) }
        })
        .await;

        assert_eq!(requests, MAX_CONSECUTIVE_FAILURES);
        assert_eq!(resolutions.len(), 8);
        assert!(resolutions.iter().all(|r| r.lookup_failed));
        let keys: Vec<&str> = resolutions.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec!["k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7"]);
    }

    #[tokio::test]
    async fn a_success_resets_the_run_of_failures() {
        let mut requests = 0;
        let queries: Vec<StockInstrumentQuery> = (0..8)
            .map(|i| query(&format!("k{i}"), Some(&format!("S{i}")), None, None))
            .collect();
        // every third lookup works
        let resolutions = resolve_without_quotes(queries, NO_DELAY, |term| {
            requests += 1;
            let works = requests % 3 == 0;
            async move {
                if works {
                    Ok(vec![found(&term)])
                } else {
                    Err(AppError::ExternalApi("rate limit".into()))
                }
            }
        })
        .await;
        assert_eq!(requests, 8, "never three failures in a row");
        assert_eq!(resolutions.iter().filter(|r| r.lookup_failed).count(), 6);
    }

    #[tokio::test]
    async fn at_most_fifty_instruments_are_looked_up_per_call() {
        let mut requests = 0;
        let queries: Vec<StockInstrumentQuery> = (0..MAX_QUERIES + 5)
            .map(|i| query(&format!("k{i}"), Some(&format!("S{i}")), None, None))
            .collect();
        let resolutions = resolve_without_quotes(queries, NO_DELAY, |term| {
            requests += 1;
            async move { Ok(vec![found(&term)]) }
        })
        .await;

        assert_eq!(requests, MAX_QUERIES);
        assert_eq!(resolutions.len(), MAX_QUERIES + 5);
        assert!(resolutions[..MAX_QUERIES]
            .iter()
            .all(|r| !r.lookup_failed && r.best.is_some()));
        assert!(resolutions[MAX_QUERIES..]
            .iter()
            .all(|r| r.lookup_failed && r.best.is_none()));
    }

    #[tokio::test]
    async fn requests_are_spaced_by_the_delay_quote_lookups_included() {
        let delay = Duration::from_millis(20);
        let started = Instant::now();
        resolve_with(
            vec![
                query("a", Some("A"), None, None),
                query("b", Some("B"), None, None),
                query("c", Some("C"), None, None),
            ],
            delay,
            |term| async move { Ok(vec![found(&term)]) },
            no_quote,
        )
        .await;
        assert!(
            started.elapsed() >= delay * 5,
            "three searches and three quotes wait five times: {:?}",
            started.elapsed()
        );
    }

    // ---- the quote lookup --------------------------------------------------------------

    /// One instrument: the search finds `listing`, the quote lookup answers `answer` (`Err`: it
    /// failed).
    async fn resolve_one(
        listing: &'static str,
        answer: std::result::Result<Option<&'static str>, ()>,
    ) -> StockInstrumentResolution {
        resolve_with(
            vec![query("k", Some("SYM"), None, None)],
            NO_DELAY,
            |_| async move { Ok(vec![found(listing)]) },
            |_| async move {
                answer
                    .map(|code| code.map(str::to_string))
                    .map_err(|()| AppError::ExternalApi("offline".into()))
            },
        )
        .await
        .remove(0)
    }

    fn best_currency(resolution: &StockInstrumentResolution) -> Option<&str> {
        resolution.best.as_ref().map(|c| c.currency.as_str())
    }

    #[test]
    fn a_listing_is_shown_in_the_currency_its_quote_reports() {
        assert_eq!(listing_currency(Some("GBP"), "VUSA.L"), "GBP");
        assert_eq!(listing_currency(Some("USD"), "CSPX.L"), "USD");
        assert_eq!(listing_currency(Some("eur"), "SXR8.DE"), "EUR");
        // Pence are GBX, whichever way Yahoo spells them.
        assert_eq!(listing_currency(Some("GBp"), "BARC.L"), "GBX");
        assert_eq!(listing_currency(Some("GBX"), "BARC.L"), "GBX");
        // The other minor units are shown as their currency.
        assert_eq!(listing_currency(Some("ZAc"), "NPN.JO"), "ZAR");
        assert_eq!(listing_currency(Some("ILA"), "TEVA.TA"), "ILS");
        // Nothing reported: the suffix guess.
        assert_eq!(listing_currency(None, "VUSA.L"), "GBP");
        assert_eq!(listing_currency(Some(" "), "SXR8.DE"), "EUR");
    }

    #[tokio::test]
    async fn the_chosen_listings_currency_is_the_one_its_quote_reports() {
        // Degiro's ISIN IE00B3XXRP09 is VUSD on the London exchange, and it trades in dollars.
        let mut quoted: Vec<String> = Vec::new();
        let resolutions = resolve_with(
            vec![query(
                "isin:IE00B3XXRP09",
                None,
                Some("IE00B3XXRP09"),
                Some("USD"),
            )],
            NO_DELAY,
            |_| async { Ok(vec![found("VUSD.L")]) },
            |symbol| {
                quoted.push(symbol);
                async { Ok(Some("USD".to_string())) }
            },
        )
        .await;

        assert_eq!(quoted, vec!["VUSD.L"]);
        let r = &resolutions[0];
        assert!(!r.lookup_failed);
        let best = r.best.as_ref().expect("a best listing");
        assert_eq!(
            (best.symbol.as_str(), best.currency.as_str()),
            ("VUSD.L", "USD")
        );
        assert_eq!(r.candidates[0].currency, "USD", "the list shows the same");
    }

    #[tokio::test]
    async fn the_unit_a_quote_reports_decides_the_code() {
        // (listing, reported by Yahoo, shown as)
        for (listing, reported, shown) in [
            ("VUSA.L", "GBP", "GBP"),
            ("CSPX.L", "USD", "USD"),
            ("SXR8.DE", "EUR", "EUR"),
            ("BARC.L", "GBp", "GBX"),
            ("LLOY.L", "GBX", "GBX"),
            ("NPN.JO", "ZAc", "ZAR"),
        ] {
            let r = resolve_one(listing, Ok(Some(reported))).await;
            assert_eq!(best_currency(&r), Some(shown), "{listing} {reported}");
        }
    }

    #[tokio::test]
    async fn a_failed_quote_keeps_the_suffix_guess_and_the_instrument_stays_verified() {
        let r = resolve_one("VUSD.L", Err(())).await;
        assert!(!r.lookup_failed, "the symbol was found all the same");
        assert_eq!(best_currency(&r), Some("GBP"), "the guess");
        assert_eq!(r.candidates.len(), 1);
    }

    #[tokio::test]
    async fn a_quote_that_names_no_currency_keeps_the_suffix_guess() {
        let r = resolve_one("SXR8.DE", Ok(None)).await;
        assert!(!r.lookup_failed);
        assert_eq!(best_currency(&r), Some("EUR"));
    }

    #[tokio::test]
    async fn only_the_chosen_listing_is_asked_for_its_quote() {
        // The trades are in euros, so VUSA.AS is chosen; only that listing costs a request.
        let mut quoted: Vec<String> = Vec::new();
        let resolutions = resolve_with(
            vec![query("symbol:VUSA", Some("VUSA"), None, Some("EUR"))],
            NO_DELAY,
            |_| async { Ok(vec![found("VUSA.L"), found("VUSA.AS"), found("VUSA.DE")]) },
            |symbol| {
                quoted.push(symbol);
                async { Ok(Some("EUR".to_string())) }
            },
        )
        .await;

        assert_eq!(quoted, vec!["VUSA.AS"]);
        let currencies: Vec<(&str, &str)> = resolutions[0]
            .candidates
            .iter()
            .map(|c| (c.symbol.as_str(), c.currency.as_str()))
            .collect();
        assert_eq!(
            currencies,
            vec![("VUSA.L", "GBP"), ("VUSA.AS", "EUR"), ("VUSA.DE", "EUR")],
            "the other listings keep the suffix guess"
        );
    }

    #[tokio::test]
    async fn the_ranking_still_compares_the_trade_currency_with_the_suffix_guesses() {
        // Known limit, kept to bound the requests: the listing is chosen by the guessed currency
        // of every candidate, and only the winner's quote is asked. Here the trades are in
        // dollars, no listing is guessed to be in dollars, so the first one wins (VUSD.AS) even
        // though VUSD.L is the dollar listing.
        let mut quoted: Vec<String> = Vec::new();
        let resolutions = resolve_with(
            vec![query("symbol:VUSD", Some("VUSD"), None, Some("USD"))],
            NO_DELAY,
            |_| async { Ok(vec![found("VUSD.AS"), found("VUSD.L")]) },
            |symbol| {
                quoted.push(symbol);
                async { Ok(Some("EUR".to_string())) }
            },
        )
        .await;

        assert_eq!(quoted, vec!["VUSD.AS"]);
        assert_eq!(
            resolutions[0].best.as_ref().map(|c| c.symbol.as_str()),
            Some("VUSD.AS")
        );
    }

    #[tokio::test]
    async fn an_instrument_without_a_chosen_listing_asks_for_no_quote() {
        let mut quotes = 0;
        let resolutions = resolve_with(
            vec![
                query("a", Some("AAA"), None, None),
                query("b", Some("BBB"), None, None),
                query("c", None, None, None),
            ],
            NO_DELAY,
            |term| async move {
                if term == "AAA" {
                    Err(AppError::ExternalApi("offline".into()))
                } else {
                    Ok(vec![])
                }
            },
            |_| {
                quotes += 1;
                async { Ok(Some("USD".to_string())) }
            },
        )
        .await;

        assert_eq!(
            quotes, 0,
            "a failed search, nothing found, nothing to search"
        );
        assert!(resolutions[0].lookup_failed);
        assert!(resolutions.iter().all(|r| r.best.is_none()));
    }

    #[tokio::test]
    async fn after_a_run_of_failed_quotes_no_more_quotes_are_asked() {
        let mut searches = 0;
        let mut quotes = 0;
        let queries: Vec<StockInstrumentQuery> = (0..8)
            .map(|i| query(&format!("k{i}"), Some(&format!("S{i}")), None, None))
            .collect();
        let resolutions = resolve_with(
            queries,
            NO_DELAY,
            |term| {
                searches += 1;
                async move { Ok(vec![found(&term)]) }
            },
            |_| {
                quotes += 1;
                async { Err(AppError::ExternalApi("rate limit".into())) }
            },
        )
        .await;

        assert_eq!(quotes, MAX_CONSECUTIVE_FAILURES);
        assert_eq!(searches, 8, "the symbol checks go on without them");
        assert!(resolutions
            .iter()
            .all(|r| !r.lookup_failed && r.best.is_some()));
    }

    #[tokio::test]
    async fn a_quote_that_works_resets_the_run_of_failed_quotes() {
        let mut quotes = 0;
        // German listings: the guess is EUR, so the dollars come from a quote that worked.
        let queries: Vec<StockInstrumentQuery> = (0..8)
            .map(|i| query(&format!("k{i}"), Some(&format!("S{i}.DE")), None, None))
            .collect();
        // every third quote works
        let resolutions = resolve_with(
            queries,
            NO_DELAY,
            |term| async move { Ok(vec![found(&term)]) },
            |_| {
                quotes += 1;
                let works = quotes % 3 == 0;
                async move {
                    if works {
                        Ok(Some("USD".to_string()))
                    } else {
                        Err(AppError::ExternalApi("rate limit".into()))
                    }
                }
            },
        )
        .await;

        assert_eq!(quotes, 8, "never three failures in a row");
        let usd = resolutions
            .iter()
            .filter(|r| best_currency(r) == Some("USD"))
            .count();
        assert_eq!(usd, 2, "the two that worked");
    }

    #[tokio::test]
    async fn at_most_fifty_quotes_are_looked_up_per_call() {
        let mut quotes = 0;
        let queries: Vec<StockInstrumentQuery> = (0..MAX_QUERIES + 5)
            .map(|i| query(&format!("k{i}"), Some(&format!("S{i}")), None, None))
            .collect();
        resolve_with(
            queries,
            NO_DELAY,
            |term| async move { Ok(vec![found(&term)]) },
            |_| {
                quotes += 1;
                async { Ok(Some("USD".to_string())) }
            },
        )
        .await;
        assert_eq!(
            quotes, MAX_QUERIES,
            "one per instrument looked up, none beyond the cap"
        );
    }

    #[test]
    fn the_public_lookup_can_be_awaited_from_a_command() {
        // Tauri commands need a Send future.
        fn assert_send<T: Send>(_: T) {}
        assert_send(resolve_instruments(Vec::new()));
    }

    #[tokio::test]
    async fn an_empty_call_resolves_nothing_and_asks_nothing() {
        assert!(resolve_instruments(Vec::new()).await.is_empty());
    }
}
