//! Stock column roles and multilingual header patterns; type-value keywords.
//!
//! Headers are compared in the normalised form of the bank import
//! ([`normalize_header`]: lowercase, diacritics folded, text in brackets and
//! punctuation dropped), so one list covers `Počet`, `Pocet` and `POČET`, and
//! "Currency (Price / share)" is simply `currency`. Patterns exist for EN, CS,
//! DE, NL, FR, ES, IT and PL plus the English headers of the brokers.
//!
//! Every role has `exact` patterns (the whole header, confidence 0.95) and
//! `contains` words (the header holds them as whole words, 0.85), minus
//! `exclude` words that rule the second tier out ("settlement date" is not the
//! trade date, "close price" is not the price). Adding a language = add its
//! words to the lists below and a case to the tests; patterns are written
//! against *normalised* text.
//!
//! Every header is claimed by at most one role and every role takes one
//! header: all matches are ranked by confidence, then role order, then column
//! (the leftmost wins a tie) and assigned greedily.

use std::sync::LazyLock;

use regex::Regex;

use super::types::{StockColumnSuggestion, TypeValueAction};
use crate::services::csv_import::columns::normalize_header;

/// What a column of a stock export means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StockRole {
    Date,
    Type,
    Symbol,
    Isin,
    Name,
    Quantity,
    Price,
    Currency,
    ExternalId,
    Fee,
}

impl StockRole {
    /// The order of the suggestions and of ties between roles.
    pub const ALL: [StockRole; 10] = [
        StockRole::Date,
        StockRole::Type,
        StockRole::Symbol,
        StockRole::Isin,
        StockRole::Name,
        StockRole::Quantity,
        StockRole::Price,
        StockRole::Currency,
        StockRole::ExternalId,
        StockRole::Fee,
    ];

    /// Key in `StockColumnSuggestion.role`.
    pub fn key(self) -> &'static str {
        match self {
            StockRole::Date => "date",
            StockRole::Type => "type",
            StockRole::Symbol => "symbol",
            StockRole::Isin => "isin",
            StockRole::Name => "name",
            StockRole::Quantity => "quantity",
            StockRole::Price => "price",
            StockRole::Currency => "currency",
            StockRole::ExternalId => "externalId",
            StockRole::Fee => "fee",
        }
    }
}

/// Confidence of a whole-header match and of a header that contains a word.
const CONF_EXACT: f64 = 0.95;
const CONF_CONTAINS: f64 = 0.85;

/// Patterns of one role (normalised text, see the module docs).
struct RoleSpec {
    role: StockRole,
    exact: &'static [&'static str],
    contains: &'static [&'static str],
    exclude: &'static [&'static str],
}

static SPECS: &[RoleSpec] = &[
    RoleSpec {
        role: StockRole::Date,
        exact: &[
            "date",
            "datum",
            "data",
            "fecha",
            "trade date",
            "tradedate",
            "transaction date",
            "execution date",
            "date executed",
            "order date",
            "date time",
            "datetime",
            "timestamp",
            "trade date time",
            // CZ
            "datum obchodu",
            "datum transakce",
            "datum provedeni",
            "datum a cas",
            // DE
            "handelsdatum",
            "handelstag",
            "ausfuhrungsdatum",
            "ausfuhrungstag",
            "transaktionsdatum",
            // NL
            "transactiedatum",
            "datum transactie",
            "uitvoeringsdatum",
            // FR
            "date de transaction",
            "date d execution",
            "date operation",
            "date de l operation",
            // ES
            "fecha operacion",
            "fecha de operacion",
            "fecha de ejecucion",
            "fecha de transaccion",
            // IT
            "data operazione",
            "data transazione",
            "data esecuzione",
            // PL
            "data operacji",
            "data transakcji",
            "data zawarcia transakcji",
            "data realizacji",
        ],
        // A time column is the date when nothing better exists (XTB, Trading
        // 212): it holds the whole timestamp.
        contains: &[
            "date",
            "datum",
            "data",
            "fecha",
            "datetime",
            "timestamp",
            "time",
            "cas",
            "czas",
            "hora",
            "tijd",
            "zeit",
            "heure",
            "ora",
        ],
        exclude: &[
            "settle",
            "settled",
            "settlement",
            "value",
            "valuta",
            "expiry",
            "expiration",
            "maturity",
            "report",
            "birth",
            "due",
        ],
    },
    RoleSpec {
        role: StockRole::Type,
        exact: &[
            "type",
            "typ",
            "action",
            "side",
            "direction",
            "buy sell",
            "buy or sell",
            "buysell",
            "b s",
            "transaction type",
            "trade type",
            // CZ
            "operace",
            "smer",
            "druh",
            "druh operace",
            "typ operace",
            "typ transakce",
            "typ obchodu",
            // DE
            "art",
            "transaktionstyp",
            "transaktionsart",
            "kauf verkauf",
            "geschaftsart",
            // NL
            "soort",
            "transactietype",
            "type transactie",
            "transactiesoort",
            // FR / ES / IT / PL
            "operation",
            "type d operation",
            "sens",
            "tipo",
            "tipo de operacion",
            "tipo operacion",
            "tipo di operazione",
            "operazione",
            "rodzaj",
            "rodzaj transakcji",
            "typ transakcji",
            "kierunek",
        ],
        contains: &[
            "buy sell",
            "side",
            "direction",
            "smer",
            "transaction type",
            "trade type",
        ],
        exclude: &[
            "asset",
            "instrument",
            "security",
            "product",
            "account",
            "order",
            "price",
            "fee",
        ],
    },
    RoleSpec {
        role: StockRole::Symbol,
        exact: &[
            "symbol",
            "ticker",
            "instrument",
            "ticker symbol",
            "stock symbol",
            "stock ticker",
            "stock code",
            "tickersymbol",
            // CZ / DE / FR / ES / IT / NL / RO
            "symbol akcie",
            "zkratka",
            "kurzel",
            "symbole",
            "simbolo",
            "symbool",
            "simbol",
        ],
        contains: &["symbol", "ticker"],
        exclude: &["currency", "name", "type", "exchange", "description", "id"],
    },
    RoleSpec {
        role: StockRole::Isin,
        exact: &[
            "isin",
            "isin code",
            "isin kod",
            "kod isin",
            "isin number",
            "isin nummer",
            "codice isin",
            "codigo isin",
            "code isin",
            "numer isin",
        ],
        contains: &["isin"],
        exclude: &[],
    },
    RoleSpec {
        role: StockRole::Name,
        exact: &[
            "name",
            "product",
            "security",
            "company",
            "security name",
            "company name",
            "instrument name",
            "stock name",
            "asset name",
            "name of security",
            "description",
            // CZ
            "nazev",
            "produkt",
            "nazev cenneho papiru",
            "nazev akcie",
            "nazev instrumentu",
            "nazev spolecnosti",
            "nazev produktu",
            "popis",
            // DE
            "bezeichnung",
            "wertpapier",
            "wertpapiername",
            "beschreibung",
            // NL / FR / ES / IT / PL
            "naam",
            "omschrijving",
            "produit",
            "nom",
            "producto",
            "nombre",
            "descripcion",
            "prodotto",
            "nome",
            "descrizione",
            "nazwa",
            "nazwa instrumentu",
            "nazwa spolki",
            "opis",
        ],
        contains: &[],
        exclude: &[],
    },
    RoleSpec {
        role: StockRole::Quantity,
        exact: &[
            "quantity",
            "qty",
            "shares",
            "no of shares",
            "number of shares",
            "units",
            "no of units",
            "number of units",
            "volume",
            "quantity shares",
            // CZ
            "pocet",
            "pocet kusu",
            "pocet akcii",
            "mnozstvi",
            "kusy",
            // DE
            "anzahl",
            "menge",
            "stuck",
            "stucke",
            // NL / PL / ES / IT / FR
            "aantal",
            "ilosc",
            "liczba",
            "liczba akcji",
            "cantidad",
            "numero de acciones",
            "quantita",
            "quantite",
            "nombre de titres",
            "nombre d actions",
        ],
        contains: &[
            "quantity", "qty", "shares", "pocet", "mnozstvi", "anzahl", "aantal", "ilosc",
            "cantidad", "quantita", "quantite",
        ],
        exclude: &["remaining", "original", "total", "cumulative", "open"],
    },
    RoleSpec {
        role: StockRole::Price,
        exact: &[
            "price",
            "unit price",
            "price share",
            "price per share",
            "share price",
            "price per unit",
            "trade price",
            "tradeprice",
            "execution price",
            // CZ
            "cena",
            "cena za kus",
            "cena za akcii",
            "cena akcie",
            "jednotkova cena",
            // DE / NL
            "kurs",
            "preis",
            "stuckpreis",
            "koers",
            // FR / ES / IT / PL
            "prix",
            "prix unitaire",
            "precio",
            "precio unitario",
            "prezzo",
            "prezzo unitario",
            "cena jednostkowa",
            "cena za sztuke",
            "cena akcji",
        ],
        contains: &[
            "price", "cena", "preis", "prix", "precio", "prezzo", "koers",
        ],
        exclude: &[
            "limit", "stop", "close", "closing", "market", "total", "last", "average", "avg",
            "currency", "exchange",
        ],
    },
    RoleSpec {
        role: StockRole::Currency,
        exact: &[
            "currency",
            "currency code",
            "ccy",
            "curr",
            "trade currency",
            "price currency",
            "currencyprimary",
            "currency primary",
            // CZ
            "mena",
            "mena obchodu",
            "mena akcie",
            // DE / NL / IT / FR / ES / PL
            "waehrung",
            "wahrung",
            "valuta",
            "devise",
            "moneda",
            "divisa",
            "munt",
            "waluta",
        ],
        contains: &[
            "currency", "ccy", "mena", "waehrung", "wahrung", "devise", "moneda", "divisa", "munt",
            "waluta",
        ],
        exclude: &[
            "fee",
            "fees",
            "commission",
            "tax",
            "conversion",
            "rate",
            "withholding",
            "charge",
            "result",
            "total",
            "base",
        ],
    },
    RoleSpec {
        role: StockRole::ExternalId,
        // A bare "id" is an id; "Account ID" or "Client ID" is not.
        exact: &[
            "id",
            "order id",
            "orderid",
            "trade id",
            "tradeid",
            "transaction id",
            "transactionid",
            "deal id",
            "execution id",
            "order number",
            "order no",
            "trade number",
            "ticket",
            "ticket id",
            // CZ
            "id objednavky",
            "id obchodu",
            "id transakce",
            "id pokynu",
            "cislo objednavky",
            "cislo obchodu",
            "cislo transakce",
            // DE
            "auftragsnummer",
            "ordernummer",
            "transaktions id",
            // FR / ES / IT / PL
            "id de transaction",
            "id de la transaccion",
            "id transazione",
            "id operazione",
            "id ordine",
            "id de orden",
            "id de la orden",
            "id de l ordre",
            "id ordre",
            "id zlecenia",
            "id transakcji",
        ],
        contains: &[
            "order id",
            "trade id",
            "transaction id",
            "orderid",
            "tradeid",
            "transactionid",
        ],
        exclude: &[
            "currency", "account", "client", "user", "customer", "security", "conid",
        ],
    },
    RoleSpec {
        role: StockRole::Fee,
        exact: &[
            "fee",
            "fees",
            "commission",
            "commissions",
            "ibcommission",
            "transaction and or third party fees",
            "transaction fee",
            "transaction fees",
            "trading fee",
            "trading fees",
            "transaction costs",
            // CZ
            "poplatek",
            "poplatky",
            "provize",
            // DE
            "gebuhr",
            "gebuhren",
            "provision",
            // NL / FR / ES / IT / PL
            "kosten",
            "transactiekosten",
            "frais",
            "comision",
            "comisiones",
            "commissione",
            "commissioni",
            "prowizja",
            "oplata",
            "oplaty",
        ],
        contains: &[
            "fee",
            "fees",
            "commission",
            "commissions",
            "poplatek",
            "poplatky",
            "provize",
            "gebuhr",
            "gebuhren",
            "kosten",
            "frais",
            "comision",
            "costes",
            "commissione",
            "commissioni",
            "costi",
            "prowizja",
            "oplata",
            "oplaty",
        ],
        exclude: &[],
    },
];

/// A role's patterns as regular expressions over normalised headers.
struct CompiledSpec {
    role: StockRole,
    exact: Regex,
    contains: Option<Regex>,
    exclude: Option<Regex>,
}

/// `(?:^| )(?:a|b)(?: |$)`: the words as whole words (or phrases).
fn words_regex(words: &[&str]) -> Option<Regex> {
    if words.is_empty() {
        return None;
    }
    let alternatives: Vec<String> = words.iter().map(|w| regex::escape(w)).collect();
    // Unreachable failure while `all_patterns_compile` is green; a bad list
    // must never take the whole import down.
    Regex::new(&format!("(?:^| )(?:{})(?: |$)", alternatives.join("|")))
        .map_err(|e| log::error!("stock column pattern for a word list is invalid: {e}"))
        .ok()
}

static COMPILED: LazyLock<Vec<CompiledSpec>> = LazyLock::new(|| {
    StockRole::ALL
        .iter()
        .filter_map(|role| {
            let spec = SPECS.iter().find(|s| s.role == *role)?;
            let alternatives: Vec<String> = spec.exact.iter().map(|p| regex::escape(p)).collect();
            let exact = Regex::new(&format!("^(?:{})$", alternatives.join("|")))
                .map_err(|e| log::error!("stock column pattern for {role:?} is invalid: {e}"))
                .ok()?;
            Some(CompiledSpec {
                role: *role,
                exact,
                contains: words_regex(spec.contains),
                exclude: words_regex(spec.exclude),
            })
        })
        .collect()
});

/// Confidence that `normalized` (already normalised) plays `spec.role`.
fn confidence(spec: &CompiledSpec, normalized: &str) -> Option<f64> {
    if spec.exact.is_match(normalized) {
        return Some(CONF_EXACT);
    }
    let holds_a_word = spec
        .contains
        .as_ref()
        .is_some_and(|re| re.is_match(normalized));
    let excluded = spec
        .exclude
        .as_ref()
        .is_some_and(|re| re.is_match(normalized));
    (holds_a_word && !excluded).then_some(CONF_CONTAINS)
}

/// Suggest a column for every role that has one, in role order.
pub fn suggest_stock_columns(headers: &[String]) -> Vec<StockColumnSuggestion> {
    let normalized: Vec<String> = headers.iter().map(|h| normalize_header(h)).collect();

    // Every (confidence, role, column) that matches, best first.
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (role_index, spec) in COMPILED.iter().enumerate() {
        for (column, text) in normalized.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            if let Some(conf) = confidence(spec, text) {
                candidates.push((conf, role_index, column));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut role_taken = vec![false; COMPILED.len()];
    let mut column_taken = vec![false; headers.len()];
    let mut chosen: Vec<(usize, StockColumnSuggestion)> = Vec::new();
    for (conf, role_index, column) in candidates {
        if role_taken[role_index] || column_taken[column] {
            continue;
        }
        role_taken[role_index] = true;
        column_taken[column] = true;
        chosen.push((
            role_index,
            StockColumnSuggestion {
                role: COMPILED[role_index].role.key().to_string(),
                column,
                confidence: conf,
            },
        ));
    }
    chosen.sort_by_key(|(role_index, _)| *role_index);
    chosen.into_iter().map(|(_, s)| s).collect()
}

/// The column suggested for `role`.
pub fn column_for(suggestions: &[StockColumnSuggestion], role: StockRole) -> Option<usize> {
    suggestions
        .iter()
        .find(|s| s.role == role.key())
        .map(|s| s.column)
}

/// The file has a fee or commission column (fees are not imported; the
/// preview says so).
pub fn has_fee_column(headers: &[String]) -> bool {
    column_for(&suggest_stock_columns(headers), StockRole::Fee).is_some()
}

/// A cell value in the folded form used to compare type values: lowercase,
/// diacritics folded, punctuation as spaces. Brackets keep their words, unlike
/// in headers: "Trade (Buy)" is a purchase.
pub fn fold_value(value: &str) -> String {
    normalize_header(&value.replace(['(', ')'], " "))
}

/// Words of a purchase and of a sale in the languages of the brokers (folded
/// form). A word matches a cell word when equal, or as a prefix for words of
/// five letters or more ("nakupu", "prodeje"), or, for the compounds of German
/// and Dutch, as prefix or suffix ("Wertpapierkauf", "aankoop").
const BUY_WORDS: [&str; 13] = [
    "buy", "bought", "purchase", "kauf", "nakup", "koupe", "zakup", "kupno", "compra", "achat",
    "acquisto", "aankoop", "koop",
];
const SELL_WORDS: [&str; 12] = [
    "sell", "sold", "sale", "verkauf", "prodej", "sprzedaz", "venta", "venda", "vende", "vente",
    "vendita", "verkoop",
];
const COMPOUND_WORDS: [&str; 4] = ["kauf", "verkauf", "koop", "verkoop"];

fn word_matches(token: &str, word: &str) -> bool {
    token == word
        || (word.len() >= 5 && token.starts_with(word))
        || (COMPOUND_WORDS.contains(&word) && (token.starts_with(word) || token.ends_with(word)))
}

fn has_word(tokens: &[&str], words: &[&str]) -> bool {
    tokens
        .iter()
        .any(|token| words.iter().any(|word| word_matches(token, word)))
}

/// What a value of the type column probably means: a purchase word → buy, a
/// sale word → sell (XTB words a sale "CLOSE BUY"), everything else (dividend,
/// deposit, withdrawal, interest, fee, tax, conversion, split, transfer, …)
/// is not a trade → skip.
pub fn suggest_type_action(value: &str) -> TypeValueAction {
    let folded = fold_value(value);
    let tokens: Vec<&str> = folded.split(' ').filter(|t| !t.is_empty()).collect();
    // "close buy" first: it holds a purchase word and is a sale. Sale words
    // before purchase words: "verkauf" ends with "kauf".
    if tokens.windows(2).any(|w| w == ["close", "buy"]) || has_word(&tokens, &SELL_WORDS) {
        TypeValueAction::Sell
    } else if has_word(&tokens, &BUY_WORDS) {
        TypeValueAction::Buy
    } else {
        TypeValueAction::Skip
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::services::stock_import::adapters::test_support::*;

    /// role key → (column, confidence)
    fn mapped(list: &[&str]) -> HashMap<String, (usize, f64)> {
        suggest_stock_columns(&headers(list))
            .into_iter()
            .map(|s| (s.role, (s.column, s.confidence)))
            .collect()
    }

    fn col(m: &HashMap<String, (usize, f64)>, role: &str) -> Option<usize> {
        m.get(role).map(|(c, _)| *c)
    }

    #[test]
    fn all_patterns_compile() {
        // A list that fails to compile is dropped with a log line at runtime;
        // this makes that a hard failure.
        assert_eq!(COMPILED.len(), StockRole::ALL.len());
        for spec in SPECS {
            assert!(!spec.exact.is_empty(), "{:?}", spec.role);
            for list in [spec.contains, spec.exclude] {
                assert_eq!(
                    words_regex(list).is_some(),
                    !list.is_empty(),
                    "{:?}",
                    spec.role
                );
            }
        }
        for (compiled, role) in COMPILED.iter().zip(StockRole::ALL) {
            assert_eq!(compiled.role, role, "SPECS follow the order of ALL");
        }
    }

    #[test]
    fn every_pattern_is_in_normalized_form() {
        // A word written with capitals, diacritics or punctuation can never
        // match a normalised header.
        for spec in SPECS {
            for list in [spec.exact, spec.contains, spec.exclude] {
                for pattern in list {
                    assert_eq!(
                        &normalize_header(pattern),
                        pattern,
                        "{:?}: {pattern:?} is not normalized",
                        spec.role
                    );
                }
            }
        }
        for word in BUY_WORDS.iter().chain(SELL_WORDS.iter()) {
            assert_eq!(&normalize_header(word), word, "{word:?}");
        }
    }

    #[test]
    fn role_keys_are_the_contract_keys() {
        let keys: Vec<&str> = StockRole::ALL.iter().map(|r| r.key()).collect();
        assert_eq!(
            keys,
            [
                "date",
                "type",
                "symbol",
                "isin",
                "name",
                "quantity",
                "price",
                "currency",
                "externalId",
                "fee"
            ]
        );
    }

    #[test]
    fn exact_headers_have_confidence_095_and_contained_words_085() {
        let m = mapped(&[
            "Date", "Type", "Ticker", "Name", "Quantity", "Price", "Currency",
        ]);
        for role in [
            "date", "type", "symbol", "name", "quantity", "price", "currency",
        ] {
            assert_eq!(m[role].1, 0.95, "{role}");
        }
        // "Time" only contains a date word; "Trade time" likewise.
        assert_eq!(mapped(&["Time"])["date"], (0, 0.85));
        assert_eq!(mapped(&["Execution time"])["date"], (0, 0.85));
        // A header that says nothing about a role gets none.
        assert!(mapped(&["Foo", "Bar", ""]).is_empty());
        assert!(suggest_stock_columns(&[]).is_empty());
    }

    #[test]
    fn moony_headers_in_english_and_czech() {
        for list in [MOONY_HEADERS_EN, MOONY_HEADERS_CS] {
            let m = mapped(&list);
            assert_eq!(col(&m, "date"), Some(0), "{list:?}");
            assert_eq!(col(&m, "type"), Some(1));
            assert_eq!(col(&m, "symbol"), Some(2));
            assert_eq!(col(&m, "name"), Some(3));
            assert_eq!(col(&m, "quantity"), Some(4));
            assert_eq!(col(&m, "price"), Some(5));
            assert_eq!(col(&m, "currency"), Some(6));
            assert_eq!(m.len(), 7);
        }
    }

    #[test]
    fn xtb_headers() {
        let m = mapped(&XTB_HEADERS);
        assert_eq!(col(&m, "externalId"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "date"), Some(2));
        assert_eq!(col(&m, "symbol"), Some(3));
        // The quantity and the price live in the comment; there is no column
        // for them and the amount is not a role.
        assert_eq!(m.len(), 4);
    }

    #[test]
    fn trading212_headers_take_the_first_of_the_repeated_currency_columns() {
        let m = mapped(&TRADING212_HEADERS);
        assert_eq!(col(&m, "type"), Some(0));
        assert_eq!(col(&m, "date"), Some(1));
        assert_eq!(col(&m, "isin"), Some(2));
        assert_eq!(col(&m, "symbol"), Some(3));
        assert_eq!(col(&m, "name"), Some(4));
        assert_eq!(col(&m, "quantity"), Some(5));
        assert_eq!(col(&m, "price"), Some(6));
        // "Currency (Price / share)" and the four other currency columns all
        // read "currency" once the brackets are gone: the leftmost is the
        // price's.
        assert_eq!(col(&m, "currency"), Some(7));
        assert_eq!(col(&m, "externalId"), Some(16));
        // The conversion fee is a fee, not a currency.
        assert_eq!(col(&m, "fee"), Some(17));
    }

    #[test]
    fn degiro_headers_with_blank_columns() {
        for list in [&DEGIRO_HEADERS[..], &DEGIRO_AUTOFX_HEADERS[..]] {
            let m = mapped(list);
            assert_eq!(col(&m, "date"), Some(0), "the date, not the time");
            assert_eq!(col(&m, "name"), Some(2));
            assert_eq!(col(&m, "isin"), Some(3));
            assert_eq!(col(&m, "quantity"), Some(6));
            assert_eq!(col(&m, "price"), Some(7));
            assert_eq!(col(&m, "externalId"), Some(list.len() - 1));
            assert!(col(&m, "fee").is_some());
            // No ticker, no type and no currency name: blanks claim nothing.
            assert!(col(&m, "symbol").is_none());
            assert!(col(&m, "type").is_none());
            assert!(col(&m, "currency").is_none());
        }
    }

    #[test]
    fn degiro_headers_in_other_languages() {
        // Dutch, Czech, German, French: names translate, the layout does not.
        let dutch = [
            "Datum",
            "Tijd",
            "Product",
            "ISIN",
            "Beurs",
            "Uitvoeringsplaats",
            "Aantal",
            "Koers",
            "",
            "Lokale waarde",
            "",
            "Waarde",
            "",
            "Wisselkoers",
            "Transactiekosten en/of kosten van derden",
            "",
            "Totaal",
            "",
            "Order ID",
        ];
        let czech = [
            "Datum",
            "Čas",
            "Produkt",
            "ISIN",
            "Referenční burza",
            "Místo provedení",
            "Počet",
            "Cena",
            "",
            "Místní hodnota",
            "",
            "Hodnota",
            "",
            "Směnný kurz",
            "Transakční poplatky a/nebo poplatky třetích stran",
            "",
            "Celkem",
            "",
            "ID objednávky",
        ];
        let german = [
            "Datum",
            "Uhrzeit",
            "Produkt",
            "ISIN",
            "Referenzbörse",
            "Handelsplatz",
            "Anzahl",
            "Kurs",
            "",
            "Lokaler Wert",
            "",
            "Wert",
            "",
            "Wechselkurs",
            "Transaktionsgebühren und/oder Gebühren von Drittparteien",
            "",
            "Gesamt",
            "",
            "Order-ID",
        ];
        let french = [
            "Date",
            "Heure",
            "Produit",
            "ISIN",
            "Bourse de référence",
            "Lieu d'exécution",
            "Quantité",
            "Cours",
            "",
            "Valeur locale",
            "",
            "Valeur",
            "",
            "Taux de change",
            "Frais de transaction et/ou de tiers",
            "",
            "Total",
            "",
            "ID de l'ordre",
        ];
        for (name, list) in [("nl", dutch), ("cs", czech), ("de", german), ("fr", french)] {
            let m = mapped(&list);
            assert_eq!(col(&m, "date"), Some(0), "{name}");
            assert_eq!(col(&m, "name"), Some(2), "{name}");
            assert_eq!(col(&m, "isin"), Some(3), "{name}");
            assert_eq!(col(&m, "quantity"), Some(6), "{name}");
            assert!(col(&m, "fee").is_some(), "{name}");
            assert_eq!(col(&m, "externalId"), Some(18), "{name}");
        }
        // "Koers"/"Cena"/"Kurs" are the price; French "Cours" is not on the list.
        assert_eq!(col(&mapped(&dutch), "price"), Some(7));
        assert_eq!(col(&mapped(&czech), "price"), Some(7));
        assert_eq!(col(&mapped(&german), "price"), Some(7));
    }

    #[test]
    fn ibkr_flex_query_headers() {
        let m = mapped(&IBKR_HEADERS);
        assert_eq!(col(&m, "symbol"), Some(0));
        assert_eq!(col(&m, "isin"), Some(1));
        assert_eq!(col(&m, "date"), Some(2));
        assert_eq!(col(&m, "type"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        assert_eq!(col(&m, "externalId"), Some(8));
        // The asset class is no role of its own.
        assert_eq!(m.len(), 8);
        let with_commission = mapped(&["TradeDate", "IBCommission", "IBCommissionCurrency"]);
        assert_eq!(col(&with_commission, "fee"), Some(1));
        assert!(!with_commission.contains_key("currency"));
    }

    #[test]
    fn other_european_languages() {
        // German
        let m = mapped(&[
            "Datum",
            "Art",
            "Symbol",
            "Bezeichnung",
            "Anzahl",
            "Kurs",
            "Währung",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        let m = mapped(&["Datum", "Typ", "Symbol", "Anzahl", "Preis", "Waehrung"]);
        assert_eq!(col(&m, "price"), Some(4));
        assert_eq!(col(&m, "currency"), Some(5));
        // Dutch
        let m = mapped(&[
            "Datum", "Soort", "Ticker", "Naam", "Aantal", "Koers", "Valuta",
        ]);
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        // French
        let m = mapped(&[
            "Date",
            "Sens",
            "Symbole",
            "Nom",
            "Quantité",
            "Prix",
            "Devise",
        ]);
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "symbol"), Some(2));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        // Spanish
        let m = mapped(&[
            "Fecha", "Tipo", "Símbolo", "Nombre", "Cantidad", "Precio", "Moneda",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "symbol"), Some(2));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        // Italian
        let m = mapped(&[
            "Data",
            "Operazione",
            "Simbolo",
            "Nome",
            "Quantità",
            "Prezzo",
            "Valuta",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "symbol"), Some(2));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        // Polish
        let m = mapped(&[
            "Data", "Rodzaj", "Symbol", "Nazwa", "Ilość", "Cena", "Waluta",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        // Czech, with the wording of a broker statement
        let m = mapped(&[
            "Datum obchodu",
            "Směr",
            "Ticker",
            "Název",
            "Množství",
            "Cena za kus",
            "Měna",
            "Poplatek",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "name"), Some(3));
        assert_eq!(col(&m, "quantity"), Some(4));
        assert_eq!(col(&m, "price"), Some(5));
        assert_eq!(col(&m, "currency"), Some(6));
        assert_eq!(col(&m, "fee"), Some(7));
    }

    #[test]
    fn broker_style_english_headers() {
        let m = mapped(&[
            "Trade Date",
            "Action",
            "Symbol",
            "Shares",
            "Unit Price",
            "CCY",
            "Commission",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "quantity"), Some(3));
        assert_eq!(col(&m, "price"), Some(4));
        assert_eq!(col(&m, "currency"), Some(5));
        assert_eq!(col(&m, "fee"), Some(6));
        let m = mapped(&[
            "Date/Time",
            "Side",
            "Instrument",
            "Qty",
            "Price",
            "Currency Code",
        ]);
        assert_eq!(col(&m, "date"), Some(0));
        assert_eq!(col(&m, "type"), Some(1));
        assert_eq!(col(&m, "symbol"), Some(2));
        assert_eq!(col(&m, "quantity"), Some(3));
        assert_eq!(col(&m, "currency"), Some(5));
    }

    #[test]
    fn a_date_beats_a_time_whatever_the_order() {
        let m = mapped(&["Time", "Date", "Ticker"]);
        assert_eq!(m["date"], (1, 0.95));
        let m = mapped(&["Date", "Time"]);
        assert_eq!(m["date"], (0, 0.95));
    }

    #[test]
    fn words_that_only_look_like_a_role_are_not_suggested() {
        // Settlement and value dates are not the trade date.
        assert!(!mapped(&["Settlement date"]).contains_key("date"));
        assert!(!mapped(&["Value date", "Valuta"]).contains_key("date"));
        // Prices of other kinds.
        assert!(!mapped(&["Close price"]).contains_key("price"));
        assert!(!mapped(&["Limit price", "Stop price"]).contains_key("price"));
        // An order type is not the direction.
        assert!(!mapped(&["Order type"]).contains_key("type"));
        assert!(!mapped(&["Asset type", "Instrument type"]).contains_key("type"));
        // The currency of a fee is not the trade currency.
        assert!(!mapped(&["Fee currency", "Currency conversion fee"]).contains_key("currency"));
        assert!(!mapped(&["Remaining quantity"]).contains_key("quantity"));
        // A bare "ID" is an id; "Account ID" and "Client ID" are not.
        assert_eq!(mapped(&["ID"])["externalId"], (0, 0.95));
        assert!(!mapped(&["Account ID", "Client ID"]).contains_key("externalId"));
    }

    #[test]
    fn every_header_is_claimed_once_and_every_role_once() {
        let m = mapped(&["Date", "Date", "Price", "Price", "Currency", "Currency"]);
        assert_eq!(m["date"].0, 0, "ties go to the leftmost header");
        assert_eq!(m["price"].0, 2);
        assert_eq!(m["currency"].0, 4);
        assert_eq!(m.len(), 3);
        // One header cannot serve two roles.
        let suggestions = suggest_stock_columns(&headers(&TRADING212_HEADERS));
        let mut columns: Vec<usize> = suggestions.iter().map(|s| s.column).collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), suggestions.len());
        // The result follows the role order, not the column order.
        let roles: Vec<&str> = suggestions.iter().map(|s| s.role.as_str()).collect();
        assert_eq!(
            roles,
            [
                "date",
                "type",
                "symbol",
                "isin",
                "name",
                "quantity",
                "price",
                "currency",
                "externalId",
                "fee"
            ]
        );
    }

    #[test]
    fn column_for_finds_a_role() {
        let s = suggest_stock_columns(&headers(&MOONY_HEADERS_EN));
        assert_eq!(column_for(&s, StockRole::Price), Some(5));
        assert_eq!(column_for(&s, StockRole::Fee), None);
    }

    #[test]
    fn fee_columns_are_found_in_every_language() {
        assert!(has_fee_column(&headers(&DEGIRO_HEADERS)));
        assert!(has_fee_column(&headers(&TRADING212_HEADERS)));
        assert!(has_fee_column(&headers(&["Date", "Poplatek"])));
        assert!(has_fee_column(&headers(&["Datum", "Provize"])));
        assert!(has_fee_column(&headers(&["Datum", "Gebühr"])));
        assert!(has_fee_column(&headers(&["Fecha", "Comisión"])));
        assert!(has_fee_column(&headers(&["AutoFX Fee"])));
        assert!(!has_fee_column(&headers(&MOONY_HEADERS_EN)));
        assert!(!has_fee_column(&headers(&IBKR_HEADERS)));
        assert!(!has_fee_column(&headers(&XTB_HEADERS)));
        assert!(!has_fee_column(&[]));
    }

    fn action(value: &str) -> TypeValueAction {
        suggest_type_action(value)
    }

    #[test]
    fn buy_words_in_eight_languages() {
        for value in [
            "Buy",
            "BUY",
            "bought",
            "Market buy",
            "Limit buy",
            "Stop limit buy",
            "BUY - MARKET",
            "OPEN BUY",
            "Stocks/ETF purchase",
            "Purchase",
            "Nákup",
            "nakup",
            "Nákup akcií",
            "Koupě",
            "koupe",
            "Kauf",
            "Wertpapierkauf",
            "Aktien/ETF-Kauf",
            "Zakup",
            "Kupno",
            "Compra",
            "Ações/ETF compra",
            "Achat",
            "Acquisto",
            "Aankoop",
            "Koop",
        ] {
            assert_eq!(action(value), TypeValueAction::Buy, "{value}");
        }
    }

    #[test]
    fn sell_words_in_eight_languages() {
        for value in [
            "Sell",
            "SELL",
            "sold",
            "Market sell",
            "Limit sell",
            "SELL - LIMIT",
            "Stocks/ETF sale",
            "Sale",
            "Verkauf",
            "Wertpapierverkauf",
            "Aktien/ETF-Verkauf",
            "Prodej",
            "Prodej akcií",
            "Sprzedaż",
            "Venta",
            "Ações/ETF vende",
            "Venda",
            "Vente",
            "Vendita",
            "Verkoop",
        ] {
            assert_eq!(action(value), TypeValueAction::Sell, "{value}");
        }
    }

    #[test]
    fn closing_a_buy_is_a_sale() {
        // XTB words a sale as "CLOSE BUY".
        assert_eq!(action("CLOSE BUY"), TypeValueAction::Sell);
        assert_eq!(action("close buy 1 @ 26.280"), TypeValueAction::Sell);
        assert_eq!(action("OPEN BUY 34 @ 11.7480"), TypeValueAction::Buy);
    }

    #[test]
    fn everything_else_is_skipped() {
        for value in [
            "Dividend",
            "Dividend (Ordinary)",
            "Deposit",
            "Withdrawal",
            "Interest on cash",
            "Free funds interests",
            "Withholding tax",
            "Currency conversion",
            "Stock split open",
            "Transfer",
            "Fee",
            "Custody fee",
            "Profit/Loss",
            "Swap",
            "Share buyback",
            "Dividenda",
            "Vklad",
            "Výběr",
            "Poplatek",
            "Dividende",
            "Einzahlung",
            "Gebühr",
            "",
            "   ",
        ] {
            assert_eq!(action(value), TypeValueAction::Skip, "{value:?}");
        }
    }

    #[test]
    fn german_sales_are_not_mistaken_for_purchases() {
        // "Verkauf" ends with "kauf".
        assert_eq!(action("VERKAUF"), TypeValueAction::Sell);
        assert_eq!(action("Wertpapier-Verkauf"), TypeValueAction::Sell);
        assert_eq!(action("Aankoop"), TypeValueAction::Buy);
        assert_eq!(action("Verkoop"), TypeValueAction::Sell);
    }

    #[test]
    fn folding_a_value_ignores_case_diacritics_and_punctuation() {
        assert_eq!(fold_value(" Nákup  AKCIÍ "), "nakup akcii");
        assert_eq!(fold_value("Stocks/ETF purchase"), "stocks etf purchase");
        // Brackets keep their words: "Trade (Buy)" is a purchase.
        assert_eq!(fold_value("Trade (Buy)"), "trade buy");
        assert_eq!(action("Trade (Buy)"), TypeValueAction::Buy);
    }
}
