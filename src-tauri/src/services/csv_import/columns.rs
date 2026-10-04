//! Header normalization and column-role detection.
//!
//! Headers are compared in a normalized form (see [`normalize_header`]) so one
//! pattern list covers diacritics, case, punctuation and "unit in brackets"
//! variants: "Amount (EUR)", "Částka", "Castka" and "ČÁSTKA" all end up as
//! `amount` / `castka`. Patterns exist for EN, CZ, DE, FR, ES, IT, NL and PL
//! plus the neobank and US/UK headers (Revolut, N26, Chase, Wise, Debit/Credit).
//!
//! Adding a language = add its words to the alternation lists below and a
//! test case in `tests`. Patterns are written against *normalized* text:
//! lowercase ASCII-ish, single spaces, no punctuation, no parenthesised text.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Normalize a header for comparison: lowercase, diacritics folded, text in
/// parentheses stripped, every run of non-alphanumerics collapsed to one space.
///
/// `"Amount (EUR)"` → `"amount"`, `"Datum zaúčtování"` → `"datum zauctovani"`,
/// `"#Příjemce/plátce"` → `"prijemce platce"`.
pub fn normalize_header(header: &str) -> String {
    // 1. Drop parenthesised text (an unclosed "(" swallows the rest).
    let mut without_parens = String::with_capacity(header.len());
    let mut depth = 0usize;
    for c in header.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => without_parens.push(c),
            _ => {}
        }
    }

    // 2. Fold diacritics (NFD + drop combining marks) and a few letters that
    //    do not decompose, then lowercase.
    let mut folded = String::with_capacity(without_parens.len());
    for c in without_parens.nfd() {
        if is_combining_mark(c) {
            continue;
        }
        for lower in c.to_lowercase() {
            match lower {
                'ł' => folded.push('l'),
                'ø' => folded.push('o'),
                'đ' => folded.push('d'),
                'ß' => folded.push_str("ss"),
                'æ' => folded.push_str("ae"),
                'œ' => folded.push_str("oe"),
                other => folded.push(other),
            }
        }
    }

    // 3. Everything that is not a letter/digit is a word separator.
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a CSV column means. The order of [`ColumnRole::ALL`] is the order in
/// which roles claim headers: more specific roles first, so "Value Date" is
/// taken by `ValueDate` before `Date` looks at the remaining headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnRole {
    TransactionId,
    ValueDate,
    Debit,
    Credit,
    Balance,
    Date,
    Amount,
    Currency,
    CounterpartyIban,
    Counterparty,
    Description,
}

impl ColumnRole {
    pub const ALL: [ColumnRole; 11] = [
        ColumnRole::TransactionId,
        ColumnRole::ValueDate,
        ColumnRole::Debit,
        ColumnRole::Credit,
        ColumnRole::Balance,
        ColumnRole::Date,
        ColumnRole::Amount,
        ColumnRole::Currency,
        ColumnRole::CounterpartyIban,
        ColumnRole::Counterparty,
        ColumnRole::Description,
    ];

    /// Key in `CsvPreviewResult.suggestedMappings`.
    ///
    /// `counterparty_iban` keeps its historical snake_case key — the shipped
    /// dialog reads it — while the roles added in CSV v2 are camelCase.
    pub fn key(self) -> &'static str {
        match self {
            ColumnRole::TransactionId => "transactionId",
            ColumnRole::ValueDate => "valueDate",
            ColumnRole::Debit => "debit",
            ColumnRole::Credit => "credit",
            ColumnRole::Balance => "balance",
            ColumnRole::Date => "date",
            ColumnRole::Amount => "amount",
            ColumnRole::Currency => "currency",
            ColumnRole::CounterpartyIban => "counterparty_iban",
            ColumnRole::Counterparty => "counterparty",
            ColumnRole::Description => "description",
        }
    }
}

/// Confidence tiers (0–1). The UI shows ≥ 0.9 as "sure", ≥ 0.7 as "probable".
const CONF_EXACT: f32 = 0.95;
const CONF_LOOSE: f32 = 0.85;
const CONF_WEAK: f32 = 0.70;

/// Patterns for one role, as regex fragments over normalized headers. Each
/// tier is joined into `^(?:a|b|c)$`, so a fragment must describe the whole
/// header. `exact` = the canonical names, `loose` = common variants,
/// `weak` = "probably", only used when nothing better exists.
struct RoleSpec {
    role: ColumnRole,
    exact: &'static [&'static str],
    loose: &'static [&'static str],
    weak: &'static [&'static str],
}

/// Optional trailing currency code ("betrag eur", "amount usd").
const CCY: &str = "(?: [a-z]{3})?";

static SPECS: &[RoleSpec] = &[
    RoleSpec {
        role: ColumnRole::TransactionId,
        exact: &[
            "transaction id",
            "transactionid",
            "transaction number",
            "transferwise id",
            "wise id",
            "tx id",
            "txn id",
            // CZ
            "id operace",
            "id transakce",
            "identifikace transakce",
            // DE
            "transaktions id",
            "transaktionsnummer",
            // PL / FR / ES / IT / NL
            "id transakcji",
            "id de transaction",
            "id de la transaccion",
            "id transazione",
            "transactie id",
        ],
        // A bare "id" or "reference" is deliberately NOT here: a row counter
        // would make every re-import look like a duplicate of the last file.
        loose: &[],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::ValueDate,
        exact: &[
            "value date",
            "valuedate",
            "valuta",
            "valuta datum",
            "valutadatum",
            "datum valuty",
            "wertstellung",
            "wertstellungsdatum",
            "fecha valor",
            "fecha de valor",
            "data valuta",
            "date de valeur",
            "date valeur",
            "waardedatum",
            "valutadatum",
            "data waluty",
            "data waluty operacji",
        ],
        loose: &[],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::Debit,
        exact: &[
            "debit",
            "debits",
            "debit amount",
            "money out",
            "paid out",
            "paid out amount",
            "payments out",
            "withdrawal",
            "withdrawals",
            "outgoing",
            "soll",
            "belastung",
            "ausgaben",
            "cargo",
            "cargos",
            "debe",
            "debit montant",
            "debito",
            "debiti",
            "uscite",
            "debet",
            "obciazenia",
            "kwota obciazenia",
            "vydaje",
            "odchozi",
            "odchozi platby",
        ],
        loose: &[],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::Credit,
        exact: &[
            "credit",
            "credits",
            "credit amount",
            "money in",
            "paid in",
            "paid in amount",
            "payments in",
            "deposit",
            "deposits",
            "incoming",
            "haben",
            "gutschrift",
            "einnahmen",
            "abono",
            "abonos",
            "haber",
            "credit montant",
            "credito",
            "crediti",
            "entrate",
            "creditering",
            "uznania",
            "kwota uznania",
            "prijmy",
            "prichozi",
            "prichozi platby",
        ],
        loose: &[],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::Balance,
        exact: &[
            "balance",
            "running balance",
            "account balance",
            "closing balance",
            "new balance",
            "ending balance",
            "balance after transaction",
            "zustatek",
            "zustatek po transakci",
            "zustatek uctu",
            "kontostand",
            "saldo",
            "saldo nach buchung",
            "solde",
            "solde apres operation",
            "saldo contable",
            "saldo contabile",
            "saldo po transakcji",
            "saldo po operacji",
        ],
        loose: &[
            "(?:balance|saldo|zustatek|kontostand|solde) .*",
            ".* (?:balance|saldo|zustatek)",
        ],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::Date,
        exact: &[
            "date",
            "datum",
            "booking date",
            "posting date",
            "post date",
            "transaction date",
            "trans date",
            "completed date",
            "date completed",
            "settled date",
            // CZ
            "datum zauctovani",
            "datum provedeni",
            "datum operace",
            "datum transakce",
            "datum zauctovani transakce",
            // DE
            "buchungstag",
            "buchungsdatum",
            "buchung",
            "datum buchung",
            // PL
            "data",
            "data operacji",
            "data ksiegowania",
            "data transakcji",
            "data waluty ksiegowania",
            // ES
            "fecha",
            "fecha operacion",
            "fecha de operacion",
            "fecha contable",
            "fecha de contabilizacion",
            // FR
            "date operation",
            "date de l operation",
            "date de comptabilisation",
            "date comptable",
            "date de transaction",
            // IT
            "data operazione",
            "data contabile",
            "data registrazione",
            "data movimento",
            "data transazione",
            // NL
            "boekingsdatum",
            "datum boeking",
            "transactiedatum",
            "datum transactie",
        ],
        loose: &[
            "started date",
            "date started",
            "execution date",
            "operation date",
            "processed date",
            "datum splatnosti",
            "datum odeslani",
            "datum zadani",
            "datum zpracovani",
            "(?:date|datum|data|fecha) .*",
            ".* (?:date|datum|data|fecha)",
        ],
        weak: &[".* (?:date|datum|data|fecha) .*"],
    },
    RoleSpec {
        role: ColumnRole::Amount,
        exact: &[
            "amount",
            "transaction amount",
            "amt",
            // CZ
            "castka",
            "castka v mene uctu",
            "zauctovana castka",
            "objem",
            "suma",
            "hodnota",
            // DE
            "betrag",
            // ES / FR / IT / NL / PL
            "importe",
            "monto",
            "montant",
            "importo",
            "bedrag",
            "kwota",
            "kwota transakcji",
            "kwota operacji",
            "kwota w walucie rachunku",
            "wartosc",
        ],
        loose: &["(?:amount|betrag|castka|importe|montant|importo|bedrag|kwota) .*"],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::Currency,
        exact: &[
            "currency",
            "currency code",
            "ccy",
            "curr",
            "mena",
            "mena uctu",
            "waehrung",
            "wahrung",
            "divisa",
            "moneda",
            "devise",
            "munt",
            "waluta",
        ],
        loose: &[],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::CounterpartyIban,
        exact: &[
            "iban",
            "counterparty iban",
            "partner iban",
            "beneficiary iban",
            "payee iban",
            "counterparty account",
            "counterparty account number",
            "payee account number",
            "payee account",
            // CZ
            "cislo protiuctu",
            "protiucet",
            "cislo uctu protistrany",
            "protiucet iban",
            // DE
            "kontonummer iban",
            "iban empfanger",
            "iban zahlungsbeteiligter",
            // ES / FR / IT / NL / PL
            "iban beneficiario",
            "iban beneficiaire",
            "iban controparte",
            "tegenrekening",
            "iban kontrahenta",
            "rachunek kontrahenta",
        ],
        loose: &[
            "account number",
            "account",
            "kontonummer",
            "cuenta",
            "compte",
            "conto",
            "rekening",
            "rekeningnummer",
            "numer rachunku",
            "(?:.* )?iban(?: .*)?",
        ],
        weak: &[],
    },
    RoleSpec {
        role: ColumnRole::Counterparty,
        exact: &[
            "counterparty",
            "counterparty name",
            "beneficiary",
            "beneficiary name",
            "payee",
            "payee name",
            "merchant",
            "merchant name",
            "partner name",
            "recipient",
            "recipient name",
            // CZ
            "prijemce",
            "protistrana",
            "nazev protiuctu",
            "nazev protistrany",
            "obchodnik",
            "prijemce platce",
            "nazev uctu protistrany",
            "odesilatel",
            "nazev prijemce",
            // DE
            "auftraggeber empfanger",
            "empfanger",
            "beguenstigter",
            "begunstigter",
            "beguenstigter zahlungspflichtiger",
            "begunstigter zahlungspflichtiger",
            "zahlungsempfanger",
            "zahlungsempfanger in",
            "zahlungspflichtiger",
            "zahlungspflichtige r",
            "auftraggeber",
            "name zahlungsbeteiligter",
            // ES / FR / IT / NL / PL
            "beneficiario",
            "ordenante",
            "beneficiaire",
            "tiers",
            "controparte",
            "naam tegenpartij",
            "tegenpartij",
            "begunstigde",
            "odbiorca",
            "nadawca",
            "nazwa odbiorcy",
            "nazwa nadawcy",
            "dane kontrahenta",
            "kontrahent",
            "nazwa kontrahenta",
        ],
        loose: &[
            "payer",
            "payer name",
            "sender",
            "sender name",
            "to from",
            "from to",
            "payee payer",
            "naam",
            "nombre",
            "nom",
        ],
        weak: &["name"],
    },
    RoleSpec {
        role: ColumnRole::Description,
        exact: &[
            "description",
            "transaction description",
            "narrative",
            "memo",
            "details of payment",
            "payment details",
            "remittance information",
            // CZ
            "popis",
            "popis transakce",
            "poznamka",
            "poznamka k platbe",
            "poznamka k uhrade",
            "zprava",
            "zprava pro prijemce",
            "ucel platby",
            "popis prikazce",
            "popis pro prijemce",
            // DE
            "verwendungszweck",
            "beschreibung",
            "zahlungsreferenz",
            "mitteilung",
            // ES / FR / IT / NL / PL
            "concepto",
            "descripcion",
            "concepto transaccion",
            "libelle",
            "libelle operation",
            "motif",
            "descrizione",
            "descrizione operazione",
            "causale",
            "omschrijving",
            "omschrijving transactie",
            "mededelingen",
            "tytul",
            "tytul operacji",
            "opis",
            "opis operacji",
            "tresc",
        ],
        loose: &[
            "buchungstext",
            "payment reference",
            "reference",
            "note",
            "notes",
            "message",
            "komentar",
            "details",
            "observaciones",
            "dettagli",
            "(?:description|popis|beschreibung|descripcion|descrizione|omschrijving|opis) .*",
        ],
        weak: &["text"],
    },
];

struct CompiledSpec {
    role: ColumnRole,
    tiers: Vec<(f32, Regex)>,
}

static COMPILED: LazyLock<Vec<CompiledSpec>> = LazyLock::new(|| {
    // The order of SPECS is the claiming order; make sure it follows ALL.
    let mut out = Vec::with_capacity(SPECS.len());
    for role in ColumnRole::ALL {
        let Some(spec) = SPECS.iter().find(|s| s.role == role) else {
            continue;
        };
        let suffix = match role {
            ColumnRole::Amount | ColumnRole::Debit | ColumnRole::Credit | ColumnRole::Balance => {
                CCY
            }
            _ => "",
        };
        let mut tiers = Vec::new();
        for (conf, list) in [
            (CONF_EXACT, spec.exact),
            (CONF_LOOSE, spec.loose),
            (CONF_WEAK, spec.weak),
        ] {
            if list.is_empty() {
                continue;
            }
            let pattern = format!("^(?:{}){}$", list.join("|"), suffix);
            match Regex::new(&pattern) {
                Ok(re) => tiers.push((conf, re)),
                // Unreachable while `all_patterns_compile` is green; a bad
                // tier must never take the whole import down.
                Err(e) => log::error!("csv column pattern for {:?} is invalid: {e}", role),
            }
        }
        out.push(CompiledSpec { role, tiers });
    }
    out
});

/// Confidence that `normalized` (already normalized) plays `spec.role`.
fn confidence(spec: &CompiledSpec, normalized: &str) -> Option<f32> {
    spec.tiers
        .iter()
        .find(|(_, re)| re.is_match(normalized))
        .map(|(conf, _)| *conf)
}

/// Pattern entry point kept for tests and the header-row scan: which role
/// (if any) would this single header take, ignoring the other headers.
#[cfg(test)]
pub fn role_of(header: &str) -> Option<(ColumnRole, f32)> {
    let normalized = normalize_header(header);
    COMPILED
        .iter()
        .filter_map(|spec| confidence(spec, &normalized).map(|c| (spec.role, c)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Suggest a column for every role. Returns `role key -> (header, confidence)`.
///
/// Each header is claimed by at most one role (in [`ColumnRole::ALL`] order),
/// the best-scoring unclaimed header wins and ties go to the leftmost one.
pub fn suggest_column_mappings(headers: &[String]) -> HashMap<String, (String, f32)> {
    let normalized: Vec<String> = headers.iter().map(|h| normalize_header(h)).collect();
    let mut claimed = vec![false; headers.len()];
    let mut mappings: HashMap<String, (String, f32)> = HashMap::new();

    for spec in COMPILED.iter() {
        let mut best: Option<(usize, f32)> = None;
        for (idx, norm) in normalized.iter().enumerate() {
            if claimed[idx] || norm.is_empty() {
                continue;
            }
            if let Some(conf) = confidence(spec, norm) {
                if best.is_none_or(|(_, best_conf)| conf > best_conf) {
                    best = Some((idx, conf));
                }
            }
        }
        if let Some((idx, conf)) = best {
            claimed[idx] = true;
            mappings.insert(spec.role.key().to_string(), (headers[idx].clone(), conf));
        }
    }

    // A file that only has a value date ("Valuta") still has dates: promote it.
    if !mappings.contains_key("date") {
        if let Some(value_date) = mappings.remove("valueDate") {
            mappings.insert("date".to_string(), value_date);
        }
    }

    mappings
}

/// Index of the header that `wanted` refers to: exact match first, then the
/// normalized form (so a preset written as "Datum zaúčtování" finds
/// "Datum zauctovani" or "DATUM ZAÚČTOVÁNÍ").
pub fn find_column(headers: &[String], wanted: &str) -> Option<usize> {
    let trimmed = wanted.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(idx) = headers.iter().position(|h| h == wanted) {
        return Some(idx);
    }
    let wanted_norm = normalize_header(trimmed);
    if wanted_norm.is_empty() {
        return None;
    }
    headers
        .iter()
        .position(|h| normalize_header(h) == wanted_norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn mapped(m: &HashMap<String, (String, f32)>, key: &str) -> Option<String> {
        m.get(key).map(|(h, _)| h.clone())
    }

    #[test]
    fn normalize_header_folds_case_diacritics_punctuation_and_brackets() {
        assert_eq!(normalize_header("Amount (EUR)"), "amount");
        assert_eq!(
            normalize_header("  Datum   zaúčtování "),
            "datum zauctovani"
        );
        assert_eq!(normalize_header("#Příjemce/plátce"), "prijemce platce");
        assert_eq!(normalize_header("BIC (SWIFT-Code)"), "bic");
        assert_eq!(normalize_header("Check or Slip #"), "check or slip");
        assert_eq!(normalize_header("\u{feff}Date"), "date");
        assert_eq!(normalize_header("Währung"), "wahrung");
        assert_eq!(normalize_header("Tytuł operacji"), "tytul operacji");
        assert_eq!(
            normalize_header("Zahlungsempfänger*in"),
            "zahlungsempfanger in"
        );
        assert_eq!(normalize_header("Kontonummer/IBAN"), "kontonummer iban");
        assert_eq!(normalize_header("Betrag (€)"), "betrag");
        assert_eq!(normalize_header("(only brackets)"), "");
        assert_eq!(normalize_header("Unclosed (EUR"), "unclosed");
        assert_eq!(normalize_header(""), "");
    }

    #[test]
    fn all_patterns_compile() {
        // A pattern that fails to compile is dropped with a log line at
        // runtime; this test makes that a hard failure instead.
        for spec in SPECS {
            for list in [spec.exact, spec.loose, spec.weak] {
                if list.is_empty() {
                    continue;
                }
                let pattern = format!("^(?:{})$", list.join("|"));
                Regex::new(&pattern)
                    .unwrap_or_else(|e| panic!("{:?}: invalid pattern: {e}", spec.role));
            }
        }
        assert_eq!(COMPILED.len(), ColumnRole::ALL.len());
        assert!(COMPILED.iter().all(|c| !c.tiers.is_empty()));
    }

    #[test]
    fn every_pattern_list_is_in_normalized_form() {
        // Alternatives written with capitals, diacritics or punctuation can
        // never match a normalized header; catch them at test time. Regex
        // syntax characters are allowed in the loose/weak lists only.
        for spec in SPECS {
            for pattern in spec.exact {
                assert_eq!(
                    &normalize_header(pattern),
                    pattern,
                    "{:?}: exact pattern {pattern:?} is not normalized",
                    spec.role
                );
            }
        }
    }

    #[test]
    fn english_headers() {
        let m = suggest_column_mappings(&headers(&["Date", "Amount", "Description"]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Date"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Amount"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Description"));
        assert!(m["date"].1 >= 0.9, "exact 'date' is high confidence");
    }

    #[test]
    fn czech_headers() {
        let m = suggest_column_mappings(&headers(&[
            "Datum zaúčtování",
            "Částka",
            "Zpráva pro příjemce",
            "VS",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Datum zaúčtování"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Částka"));
        assert_eq!(
            mapped(&m, "description").as_deref(),
            Some("Zpráva pro příjemce")
        );
    }

    #[test]
    fn german_headers_both_spellings() {
        for ae in ["Währung", "Waehrung"] {
            let m = suggest_column_mappings(&headers(&[
                "Buchungstag",
                "Valutadatum",
                "Verwendungszweck",
                "Beguenstigter/Zahlungspflichtiger",
                "Kontonummer/IBAN",
                "Betrag",
                ae,
            ]));
            assert_eq!(mapped(&m, "date").as_deref(), Some("Buchungstag"));
            assert_eq!(mapped(&m, "valueDate").as_deref(), Some("Valutadatum"));
            assert_eq!(
                mapped(&m, "description").as_deref(),
                Some("Verwendungszweck")
            );
            assert_eq!(
                mapped(&m, "counterparty").as_deref(),
                Some("Beguenstigter/Zahlungspflichtiger")
            );
            assert_eq!(
                mapped(&m, "counterparty_iban").as_deref(),
                Some("Kontonummer/IBAN")
            );
            assert_eq!(mapped(&m, "amount").as_deref(), Some("Betrag"));
            assert_eq!(mapped(&m, "currency").as_deref(), Some(ae));
        }
    }

    #[test]
    fn other_european_languages() {
        // French
        let m = suggest_column_mappings(&headers(&[
            "Date opération",
            "Libellé",
            "Montant",
            "Devise",
            "Solde",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Date opération"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Libellé"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Montant"));
        assert_eq!(mapped(&m, "currency").as_deref(), Some("Devise"));
        assert_eq!(mapped(&m, "balance").as_deref(), Some("Solde"));

        // Spanish
        let m = suggest_column_mappings(&headers(&[
            "Fecha operación",
            "Fecha valor",
            "Concepto",
            "Importe",
            "Saldo",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Fecha operación"));
        assert_eq!(mapped(&m, "valueDate").as_deref(), Some("Fecha valor"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Concepto"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Importe"));

        // Italian
        let m = suggest_column_mappings(&headers(&[
            "Data operazione",
            "Descrizione operazione",
            "Importo",
            "Divisa",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Data operazione"));
        assert_eq!(
            mapped(&m, "description").as_deref(),
            Some("Descrizione operazione")
        );
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Importo"));
        assert_eq!(mapped(&m, "currency").as_deref(), Some("Divisa"));

        // Dutch
        let m = suggest_column_mappings(&headers(&[
            "Boekingsdatum",
            "Omschrijving",
            "Bedrag",
            "Tegenrekening",
            "Naam tegenpartij",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Boekingsdatum"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Omschrijving"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Bedrag"));
        assert_eq!(
            mapped(&m, "counterparty_iban").as_deref(),
            Some("Tegenrekening")
        );
        assert_eq!(
            mapped(&m, "counterparty").as_deref(),
            Some("Naam tegenpartij")
        );

        // Polish
        let m = suggest_column_mappings(&headers(&[
            "Data księgowania",
            "Tytuł operacji",
            "Kwota",
            "Waluta",
            "Nadawca",
            "Saldo po transakcji",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Data księgowania"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Tytuł operacji"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Kwota"));
        assert_eq!(mapped(&m, "currency").as_deref(), Some("Waluta"));
        assert_eq!(mapped(&m, "counterparty").as_deref(), Some("Nadawca"));
        assert_eq!(
            mapped(&m, "balance").as_deref(),
            Some("Saldo po transakcji")
        );
    }

    #[test]
    fn revolut_prefers_completed_over_started_date() {
        let m = suggest_column_mappings(&headers(&[
            "Type",
            "Product",
            "Started Date",
            "Completed Date",
            "Description",
            "Amount",
            "Fee",
            "Currency",
            "State",
            "Balance",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Completed Date"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Amount"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Description"));
        assert_eq!(mapped(&m, "currency").as_deref(), Some("Currency"));
        assert_eq!(mapped(&m, "balance").as_deref(), Some("Balance"));
        assert!(!m.contains_key("debit") && !m.contains_key("credit"));
    }

    #[test]
    fn n26_amount_with_unit_and_partner_columns() {
        let m = suggest_column_mappings(&headers(&[
            "Booking Date",
            "Value Date",
            "Partner Name",
            "Partner Iban",
            "Type",
            "Payment Reference",
            "Account Name",
            "Amount (EUR)",
            "Original Amount",
            "Original Currency",
            "Exchange Rate",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Booking Date"));
        assert_eq!(mapped(&m, "valueDate").as_deref(), Some("Value Date"));
        assert_eq!(mapped(&m, "counterparty").as_deref(), Some("Partner Name"));
        assert_eq!(
            mapped(&m, "counterparty_iban").as_deref(),
            Some("Partner Iban")
        );
        assert_eq!(
            mapped(&m, "description").as_deref(),
            Some("Payment Reference")
        );
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Amount (EUR)"));
        assert!(
            !m.contains_key("currency"),
            "'Original Currency' belongs to the foreign amount, not the booked one"
        );
    }

    #[test]
    fn chase_posting_date_and_details_flag() {
        let m = suggest_column_mappings(&headers(&[
            "Details",
            "Posting Date",
            "Description",
            "Amount",
            "Type",
            "Balance",
            "Check or Slip #",
        ]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Posting Date"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Description"));
        assert_eq!(mapped(&m, "amount").as_deref(), Some("Amount"));
        assert_eq!(mapped(&m, "balance").as_deref(), Some("Balance"));
    }

    #[test]
    fn debit_credit_pairs() {
        for (debit, credit) in [
            ("Debit", "Credit"),
            ("Money Out", "Money In"),
            ("Paid Out", "Paid In"),
            ("Soll", "Haben"),
            ("Debit Amount", "Credit Amount"),
        ] {
            let m = suggest_column_mappings(&headers(&["Date", "Description", debit, credit]));
            assert_eq!(mapped(&m, "debit").as_deref(), Some(debit), "{debit}");
            assert_eq!(mapped(&m, "credit").as_deref(), Some(credit), "{credit}");
            assert!(!m.contains_key("amount"));
        }
    }

    #[test]
    fn wise_headers_prefer_payee_and_map_transaction_id() {
        let m = suggest_column_mappings(&headers(&[
            "TransferWise ID",
            "Date",
            "Amount",
            "Currency",
            "Description",
            "Payment Reference",
            "Running Balance",
            "Exchange From",
            "Exchange To",
            "Exchange Rate",
            "Payer Name",
            "Payee Name",
            "Payee Account Number",
            "Merchant",
        ]));
        assert_eq!(
            mapped(&m, "transactionId").as_deref(),
            Some("TransferWise ID")
        );
        assert_eq!(mapped(&m, "date").as_deref(), Some("Date"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Description"));
        assert_eq!(mapped(&m, "balance").as_deref(), Some("Running Balance"));
        assert_eq!(mapped(&m, "counterparty").as_deref(), Some("Payee Name"));
        assert_eq!(
            mapped(&m, "counterparty_iban").as_deref(),
            Some("Payee Account Number")
        );
    }

    #[test]
    fn bare_id_column_is_never_a_transaction_id() {
        let m = suggest_column_mappings(&headers(&["ID", "Date", "Amount", "Reference"]));
        assert!(!m.contains_key("transactionId"));
        assert_eq!(mapped(&m, "description").as_deref(), Some("Reference"));
    }

    #[test]
    fn value_date_only_is_promoted_to_date() {
        let m = suggest_column_mappings(&headers(&["Valuta", "Betrag", "Verwendungszweck"]));
        assert_eq!(mapped(&m, "date").as_deref(), Some("Valuta"));
        assert!(!m.contains_key("valueDate"));
    }

    #[test]
    fn own_account_number_is_not_the_counterparty_account() {
        let m = suggest_column_mappings(&headers(&[
            "Číslo účtu",
            "Datum zaúčtování",
            "Částka",
            "Číslo protiúčtu",
        ]));
        assert_eq!(
            mapped(&m, "counterparty_iban").as_deref(),
            Some("Číslo protiúčtu")
        );
    }

    #[test]
    fn a_header_is_claimed_by_one_role_only() {
        let m = suggest_column_mappings(&headers(&["Value Date", "Amount"]));
        // Only a value date exists, so it becomes the date; it must not also
        // stay behind as a value date.
        assert_eq!(mapped(&m, "date").as_deref(), Some("Value Date"));
        assert!(!m.contains_key("valueDate"));
        let values: Vec<&String> = m.values().map(|(h, _)| h).collect();
        let mut dedup = values.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(values.len(), dedup.len());
    }

    #[test]
    fn empty_and_unknown_headers_map_nothing() {
        assert!(suggest_column_mappings(&[]).is_empty());
        assert!(suggest_column_mappings(&headers(&["", "foo", "bar"])).is_empty());
    }

    #[test]
    fn find_column_exact_then_normalized() {
        let h = headers(&["Datum zauctovani", "Částka", "Amount (EUR)"]);
        assert_eq!(find_column(&h, "Částka"), Some(1));
        assert_eq!(find_column(&h, "Datum zaúčtování"), Some(0));
        assert_eq!(find_column(&h, "ČÁSTKA"), Some(1));
        assert_eq!(find_column(&h, "Amount"), Some(2));
        assert_eq!(find_column(&h, "missing"), None);
        assert_eq!(find_column(&h, "  "), None);
    }

    #[test]
    fn role_of_reports_confidence() {
        let (role, conf) = role_of("Posting Date").unwrap();
        assert_eq!(role, ColumnRole::Date);
        assert!(conf >= 0.9);
        assert!(role_of("Check or Slip #").is_none());
    }
}
