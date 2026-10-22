//! Bank CSV presets: data-driven column mappings for known statement formats.
//!
//! Presets are JSON files under `src-tauri/resources/csv-presets/`, embedded at
//! compile time with `include_str!` (a missing file is a build error, not a
//! silent no-op), exactly like the categorization rule packs. The
//! `all_presets_are_valid` test in this module is the contribution gate — see
//! "Adding a bank preset" in `CONTRIBUTING.md`.
//!
//! A preset is applied in two ways:
//! - **auto-detected** from the header row (`detect_preset`): every
//!   `matchHeaders` entry must be present (normalized comparison), more
//!   `matchHeaders` wins a tie between two presets;
//! - **picked by the user** (`get_preset`).
//!
//! Column names in a preset are looked up with the same normalization as the
//! generic column patterns, so "Datum zaúčtování" in a preset finds
//! "DATUM ZAUCTOVANI" in a file.

use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::csv_import::columns::{find_column, normalize_header};

/// Keep only rows whose `column` equals `equals` (case- and whitespace-
/// insensitive). Revolut: `State == COMPLETED` drops PENDING/REVERTED rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct CsvRowFilter {
    pub column: String,
    pub equals: String,
}

/// Where a bank hands out its statement file on a phone: in its own app, or in its internet
/// banking opened in the phone's browser (wire type — TS `ExportRoute`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ExportRoute {
    App,
    Web,
}

/// How to get this bank's statement file onto a phone (spec §8.6, rung 2; wire type — TS
/// `MobileExportGuide`). Data, like the rest of the preset: the guide ships in the preset file and
/// is shown, never executed. `steps` is locale → two to four short imperative steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct MobileExportGuide {
    pub route: ExportRoute,
    /// What the person picks in the bank ("CSV"; Revolut's personal statement offers "Excel (CSV)").
    pub format: String,
    /// Locale code (`en`, `cs`, …) → the steps; every locale the app ships is present.
    pub steps: BTreeMap<String, Vec<String>>,
    /// A bare URL scheme (`fio`, never `fio://`) that opens the bank's own app. No bank documents
    /// one today; it stays `None` until one does.
    #[serde(rename = "appUrlScheme", default)]
    pub app_url_scheme: Option<String>,
    /// The bank's internet-banking entry (`https://…`); required for a `web` route.
    #[serde(rename = "webUrl", default)]
    pub web_url: Option<String>,
}

/// One bank's CSV format (wire type — TS `CsvPreset`). `schemaVersion: 1`.
///
/// Exactly one of `amountColumn` or the `debitColumn` + `creditColumn` pair is
/// set. Optional text fields are `null` on the wire when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct CsvPreset {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    /// Stable kebab-case id, also the file name (`revolut` → `revolut.json`).
    pub id: String,
    #[serde(rename = "bankName")]
    pub bank_name: String,
    /// ISO 3166-1 alpha-2 country of the bank (the UI groups/sorts by it).
    pub country: String,
    /// Row id in the `institutions` table, when the bank is seeded there
    /// (lets the dialog preselect the preset from the account's institution).
    #[serde(rename = "institutionId", default)]
    pub institution_id: Option<String>,
    /// Header cells that must all be present for auto-detection.
    #[serde(rename = "matchHeaders")]
    pub match_headers: Vec<String>,
    /// One character: `,` `;` or a tab (`"\t"`).
    pub delimiter: String,
    /// Encoding label (`UTF-8`, `Windows-1250`, …). Only consulted when the
    /// file is not valid UTF-8 — see `decode_csv_content_for_country`.
    pub encoding: String,
    #[serde(rename = "dateColumn")]
    pub date_column: String,
    /// chrono format of the date column (`%d.%m.%Y`, `%Y-%m-%d %H:%M:%S`).
    #[serde(rename = "dateFormat")]
    pub date_format: String,
    #[serde(rename = "amountColumn", default)]
    pub amount_column: Option<String>,
    #[serde(rename = "debitColumn", default)]
    pub debit_column: Option<String>,
    #[serde(rename = "creditColumn", default)]
    pub credit_column: Option<String>,
    /// Joined with " | " in this order into the transaction description.
    #[serde(rename = "descriptionColumns")]
    pub description_columns: Vec<String>,
    #[serde(rename = "counterpartyColumn", default)]
    pub counterparty_column: Option<String>,
    #[serde(rename = "counterpartyIbanColumn", default)]
    pub counterparty_iban_column: Option<String>,
    #[serde(rename = "counterpartyCountryColumn", default)]
    pub counterparty_country_column: Option<String>,
    #[serde(rename = "counterpartyBankCodeColumn", default)]
    pub counterparty_bank_code_column: Option<String>,
    #[serde(rename = "currencyColumn", default)]
    pub currency_column: Option<String>,
    #[serde(rename = "balanceColumn", default)]
    pub balance_column: Option<String>,
    /// A column that is unique per transaction at the bank (Fio "ID operace");
    /// feeds the bank-side-id duplicate rule.
    #[serde(rename = "transactionIdColumn", default)]
    pub transaction_id_column: Option<String>,
    #[serde(rename = "rowFilter", default)]
    pub row_filter: Option<CsvRowFilter>,
    #[serde(rename = "exportHelpUrl", default)]
    pub export_help_url: Option<String>,
    /// File name in `src-tauri/tests/fixtures/csv/` that this
    /// preset must import without errors (checked by `all_presets_are_valid`).
    #[serde(rename = "sampleFixture", default)]
    pub sample_fixture: Option<String>,
    /// How to get this bank's statement file on a phone (the phone app's import sheet shows it).
    /// Every bank whose phone export path is documented carries one; the generic layouts do not,
    /// nor does Moneta until a device check (no CSV export was found).
    #[serde(rename = "mobileExport", default)]
    pub mobile_export: Option<MobileExportGuide>,
}

/// Every embedded preset source. Adding a preset = add the JSON file and one
/// line here; `all_presets_are_valid` enforces the schema.
pub const PRESET_SOURCES: &[&str] = &[
    include_str!("../../resources/csv-presets/revolut.json"),
    include_str!("../../resources/csv-presets/wise.json"),
    include_str!("../../resources/csv-presets/n26.json"),
    include_str!("../../resources/csv-presets/fio.json"),
    include_str!("../../resources/csv-presets/ceska-sporitelna.json"),
    include_str!("../../resources/csv-presets/komercni-banka.json"),
    include_str!("../../resources/csv-presets/air-bank.json"),
    include_str!("../../resources/csv-presets/csob.json"),
    include_str!("../../resources/csv-presets/mbank.json"),
    include_str!("../../resources/csv-presets/raiffeisenbank.json"),
    include_str!("../../resources/csv-presets/moneta.json"),
    include_str!("../../resources/csv-presets/uk-debit-credit.json"),
    include_str!("../../resources/csv-presets/chase.json"),
    include_str!("../../resources/csv-presets/sparkasse.json"),
    include_str!("../../resources/csv-presets/dkb.json"),
    include_str!("../../resources/csv-presets/ing-de.json"),
];

/// Parse one preset source and check its internal consistency. Returns a
/// descriptive error for the validation test; runtime callers go through
/// [`all_presets`].
pub fn parse_preset(source: &str) -> Result<CsvPreset, String> {
    let preset: CsvPreset =
        serde_json::from_str(source).map_err(|e| format!("invalid preset JSON: {e}"))?;
    validate_preset(&preset)?;
    Ok(preset)
}

fn validate_preset(p: &CsvPreset) -> Result<(), String> {
    let id = &p.id;
    let fail = |msg: &str| Err(format!("preset '{id}': {msg}"));

    if p.schema_version != 1 {
        return fail(&format!("unsupported schemaVersion {}", p.schema_version));
    }
    if p.id.is_empty()
        || !p
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return fail("id must be kebab-case ([a-z0-9-]+)");
    }
    if p.bank_name.trim().is_empty() {
        return fail("bankName is empty");
    }
    if p.country.len() != 2 || !p.country.chars().all(|c| c.is_ascii_uppercase()) {
        return fail("country must be an ISO 3166-1 alpha-2 code (e.g. CZ)");
    }
    if p.delimiter.chars().count() != 1 || ![",", ";", "\t"].contains(&p.delimiter.as_str()) {
        return fail("delimiter must be ',' ';' or a tab");
    }
    if encoding_rs::Encoding::for_label(p.encoding.as_bytes()).is_none() {
        return fail(&format!("unknown encoding '{}'", p.encoding));
    }
    if chrono::format::StrftimeItems::new(&p.date_format)
        .any(|item| matches!(item, chrono::format::Item::Error))
    {
        return fail(&format!(
            "dateFormat '{}' is not a chrono format",
            p.date_format
        ));
    }
    if !p.date_format.contains("%Y") && !p.date_format.contains("%y") {
        return fail("dateFormat has no year");
    }

    let has_pair = p.debit_column.is_some() && p.credit_column.is_some();
    let has_half_pair = p.debit_column.is_some() != p.credit_column.is_some();
    if has_half_pair {
        return fail("debitColumn and creditColumn must be set together");
    }
    if p.amount_column.is_some() == has_pair {
        return fail("set amountColumn or the debitColumn + creditColumn pair, not both/neither");
    }
    if p.description_columns.is_empty() {
        return fail("descriptionColumns is empty");
    }

    // Auto-detection must be enough to import: the columns the import cannot do
    // without have to be part of what detection matched on.
    if p.match_headers.is_empty() {
        return fail("matchHeaders is empty");
    }
    let matched: HashSet<String> = p
        .match_headers
        .iter()
        .map(|h| normalize_header(h))
        .collect();
    if matched.len() != p.match_headers.len() || matched.contains("") {
        return fail("matchHeaders has an empty or duplicate entry");
    }
    let required: Vec<(&str, Option<&String>)> = vec![
        ("dateColumn", Some(&p.date_column)),
        ("amountColumn", p.amount_column.as_ref()),
        ("debitColumn", p.debit_column.as_ref()),
        ("creditColumn", p.credit_column.as_ref()),
        ("rowFilter.column", p.row_filter.as_ref().map(|f| &f.column)),
    ];
    for (field, value) in required {
        if let Some(value) = value {
            if !matched.contains(&normalize_header(value)) {
                return fail(&format!(
                    "{field} '{value}' must also be listed in matchHeaders"
                ));
            }
        }
    }

    for col in p
        .description_columns
        .iter()
        .chain(p.counterparty_column.iter())
        .chain(p.counterparty_iban_column.iter())
        .chain(p.currency_column.iter())
        .chain(p.balance_column.iter())
        .chain(p.transaction_id_column.iter())
    {
        if normalize_header(col).is_empty() {
            return fail("a column name is empty");
        }
    }
    if let Some(filter) = &p.row_filter {
        if filter.equals.trim().is_empty() {
            return fail("rowFilter.equals is empty");
        }
    }
    if let Some(url) = &p.export_help_url {
        if !url.starts_with("https://") {
            return fail("exportHelpUrl must be an https:// URL");
        }
    }
    if let Some(fixture) = &p.sample_fixture {
        if fixture.contains('/') || fixture.contains('\\') || !fixture.ends_with(".csv") {
            return fail("sampleFixture must be a bare .csv file name");
        }
    }
    if let Some(guide) = &p.mobile_export {
        validate_mobile_export(id, guide)?;
    }
    Ok(())
}

/// The languages the app ships (`src/i18n/languages.ts`): a guide has steps in every one of them,
/// because a missing locale would fall back to English on a phone set to another language.
const SHIPPED_LOCALES: [&str; 10] = ["en", "cs", "de", "es", "it", "nb", "pl", "sv", "ja", "zh"];

/// Schemes that never open a bank's app: a guide's `appUrlScheme` is handed to the system opener.
const NOT_AN_APP_SCHEME: [&str; 6] = ["http", "https", "javascript", "data", "file", "blob"];

/// `fio` — a scheme without the `:` or `://` that follows it (RFC 3986: a lowercase letter, then
/// lowercase letters, digits, `+`, `-` or `.`).
fn is_bare_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c))
}

fn validate_mobile_export(id: &str, guide: &MobileExportGuide) -> Result<(), String> {
    let fail = |msg: &str| Err(format!("preset '{id}': mobileExport: {msg}"));

    if guide.format.trim().is_empty() {
        return fail("format is empty");
    }
    for locale in SHIPPED_LOCALES {
        if !guide.steps.contains_key(locale) {
            return fail(&format!("steps has no locale '{locale}'"));
        }
    }
    for (locale, steps) in &guide.steps {
        if !SHIPPED_LOCALES.contains(&locale.as_str()) {
            return fail(&format!("steps has an unknown locale '{locale}'"));
        }
        if !(2..=4).contains(&steps.len()) {
            return fail(&format!(
                "steps for '{locale}' must have 2 to 4 entries, not {}",
                steps.len()
            ));
        }
        if steps.iter().any(|step| step.trim().is_empty()) {
            return fail(&format!("steps for '{locale}' has an empty entry"));
        }
    }
    match (&guide.web_url, guide.route) {
        (Some(url), _) if !url.starts_with("https://") || url.len() <= "https://".len() => {
            return fail("webUrl must be an https:// URL");
        }
        (None, ExportRoute::Web) => return fail("a web route needs a webUrl (https://)"),
        _ => {}
    }
    if let Some(scheme) = &guide.app_url_scheme {
        if !is_bare_scheme(scheme) || NOT_AN_APP_SCHEME.contains(&scheme.as_str()) {
            return fail(&format!(
                "appUrlScheme '{scheme}' must be a bare app scheme (fio, not fio://)"
            ));
        }
    }
    Ok(())
}

static PRESETS: LazyLock<Vec<CsvPreset>> = LazyLock::new(|| {
    PRESET_SOURCES
        .iter()
        .filter_map(|src| match parse_preset(src) {
            Ok(preset) => Some(preset),
            Err(e) => {
                // Unreachable when the validation test is green; never panic
                // in production over a bad data file.
                log::error!("Skipping invalid CSV preset: {e}");
                None
            }
        })
        .collect()
});

/// All presets shipped with the app, parsed once.
pub fn all_presets() -> &'static [CsvPreset] {
    &PRESETS
}

/// Preset by its stable id (`revolut`).
pub fn get_preset(id: &str) -> Option<&'static CsvPreset> {
    all_presets().iter().find(|p| p.id == id)
}

/// Preset for a seeded institution row (`inst_revolut`).
pub fn get_preset_by_institution(institution_id: &str) -> Option<&'static CsvPreset> {
    all_presets()
        .iter()
        .find(|p| p.institution_id.as_deref() == Some(institution_id))
}

/// The preset whose `matchHeaders` are all present in `headers` (normalized
/// comparison). When several match, the one with more `matchHeaders` wins —
/// it is the more specific — and the first listed wins a remaining tie.
pub fn detect_preset(headers: &[String]) -> Option<&'static CsvPreset> {
    let present: HashSet<String> = headers.iter().map(|h| normalize_header(h)).collect();
    all_presets()
        .iter()
        .filter(|p| {
            p.match_headers
                .iter()
                .all(|m| present.contains(&normalize_header(m)))
        })
        .fold(None, |best: Option<&CsvPreset>, p| match best {
            Some(b) if b.match_headers.len() >= p.match_headers.len() => Some(b),
            _ => Some(p),
        })
}

/// A preset's columns resolved against a concrete header row: every name is the
/// file's own spelling, ready to be used as an import mapping.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResolvedPreset {
    pub date: String,
    pub counterparty_country: Option<String>,
    pub counterparty_bank_code: Option<String>,
    pub amount: Option<String>,
    pub debit: Option<String>,
    pub credit: Option<String>,
    pub balance: Option<String>,
    pub transaction_id: Option<String>,
    pub currency: Option<String>,
    pub counterparty: Option<String>,
    pub counterparty_iban: Option<String>,
    /// Only the description columns that exist in the file, in preset order.
    pub description: Vec<String>,
    pub row_filter: Option<CsvRowFilter>,
}

/// Resolve `preset` against `headers`. `None` when the columns an import
/// cannot do without (date, amount or debit+credit) are missing — the preset
/// does not describe this file. Optional columns that are absent resolve to
/// `None` (or are dropped from the description list).
pub fn resolve_preset(preset: &CsvPreset, headers: &[String]) -> Option<ResolvedPreset> {
    let resolve = |wanted: &str| find_column(headers, wanted).map(|i| headers[i].clone());
    let resolve_opt = |wanted: &Option<String>| wanted.as_deref().and_then(resolve);

    let date = resolve(&preset.date_column)?;
    let amount = resolve_opt(&preset.amount_column);
    let debit = resolve_opt(&preset.debit_column);
    let credit = resolve_opt(&preset.credit_column);
    let amount_ok = amount.is_some() || (debit.is_some() && credit.is_some());
    if !amount_ok {
        return None;
    }

    let row_filter = match &preset.row_filter {
        Some(filter) => Some(CsvRowFilter {
            column: resolve(&filter.column)?,
            equals: filter.equals.clone(),
        }),
        None => None,
    };

    Some(ResolvedPreset {
        date,
        counterparty_country: resolve_opt(&preset.counterparty_country_column),
        counterparty_bank_code: resolve_opt(&preset.counterparty_bank_code_column),
        amount,
        debit,
        credit,
        balance: resolve_opt(&preset.balance_column),
        transaction_id: resolve_opt(&preset.transaction_id_column),
        currency: resolve_opt(&preset.currency_column),
        counterparty: resolve_opt(&preset.counterparty_column),
        counterparty_iban: resolve_opt(&preset.counterparty_iban_column),
        description: preset
            .description_columns
            .iter()
            .filter_map(|c| resolve(c))
            .collect(),
        row_filter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_embedded_preset_parses_and_is_registered() {
        assert_eq!(
            all_presets().len(),
            PRESET_SOURCES.len(),
            "a preset failed to parse and was skipped"
        );
    }

    #[test]
    fn initial_set_is_complete() {
        let ids: HashSet<&str> = all_presets().iter().map(|p| p.id.as_str()).collect();
        for expected in [
            "revolut",
            "wise",
            "n26",
            "fio",
            "ceska-sporitelna",
            "komercni-banka",
            "air-bank",
            "csob",
            "mbank",
            "raiffeisenbank",
            "moneta",
            "uk-debit-credit",
            "chase",
            "sparkasse",
            "dkb",
            "ing-de",
        ] {
            assert!(ids.contains(expected), "missing preset {expected}");
        }
    }

    #[test]
    fn ids_and_institutions_are_unique() {
        let mut ids = HashSet::new();
        let mut institutions = HashSet::new();
        for p in all_presets() {
            assert!(ids.insert(p.id.clone()), "duplicate preset id {}", p.id);
            if let Some(inst) = &p.institution_id {
                assert!(
                    institutions.insert(inst.clone()),
                    "duplicate institution {inst}"
                );
            }
        }
    }

    #[test]
    fn lookups_by_id_and_institution() {
        let revolut = get_preset("revolut").expect("revolut");
        assert_eq!(revolut.bank_name, "Revolut");
        assert_eq!(revolut.delimiter, ",");
        assert_eq!(revolut.encoding, "UTF-8");
        assert_eq!(
            get_preset_by_institution("inst_revolut").map(|p| p.id.as_str()),
            Some("revolut")
        );
        assert!(get_preset("nope").is_none());
        assert!(get_preset_by_institution("inst_nonexistent_bank").is_none());
    }

    #[test]
    fn parse_preset_rejects_inconsistent_presets() {
        let base = |extra: &str| {
            format!(
                r#"{{"schemaVersion":1,"id":"x","bankName":"X","country":"CZ","matchHeaders":["Date","Amount"],
                "delimiter":";","encoding":"UTF-8","dateColumn":"Date","dateFormat":"%d.%m.%Y",
                "amountColumn":"Amount","descriptionColumns":["Note"]{extra}}}"#
            )
        };
        assert!(parse_preset(&base("")).is_ok());
        // Unknown field
        assert!(parse_preset(&base(r#","bogus":1"#))
            .unwrap_err()
            .contains("bogus"));
        // Date column not part of matchHeaders
        let err =
            parse_preset(&base("").replace(r#"["Date","Amount"]"#, r#"["Amount"]"#)).unwrap_err();
        assert!(err.contains("matchHeaders"), "{err}");
        // Both amount and a pair
        let err = parse_preset(&base(r#","debitColumn":"D","creditColumn":"C""#)).unwrap_err();
        assert!(err.contains("not both"), "{err}");
        // Half a pair
        let err =
            parse_preset(&base(r#","debitColumn":"D""#).replace(r#""amountColumn":"Amount","#, ""))
                .unwrap_err();
        assert!(err.contains("together"), "{err}");
        // Bad delimiter, encoding, date format, id, country
        assert!(parse_preset(&base("").replace(r#"";""#, r#""|""#)).is_err());
        assert!(parse_preset(&base("").replace("UTF-8", "klingon")).is_err());
        assert!(parse_preset(&base("").replace("%d.%m.%Y", "%Q")).is_err());
        assert!(parse_preset(&base("").replace("%d.%m.%Y", "%d.%m")).is_err());
        assert!(parse_preset(&base("").replace(r#""id":"x""#, r#""id":"X Y""#)).is_err());
        assert!(parse_preset(&base("").replace(r#""CZ""#, r#""Czechia""#)).is_err());
        // Non-https help link, path-like fixture
        assert!(parse_preset(&base(r#","exportHelpUrl":"http://x.test""#)).is_err());
        assert!(parse_preset(&base(r#","sampleFixture":"../x.csv""#)).is_err());
    }

    // ---- the mobile export guide ----

    const GUIDE_LOCALES: [&str; 10] = ["en", "cs", "de", "es", "it", "nb", "pl", "sv", "ja", "zh"];

    /// The `steps` object of a guide: the same `steps` in each of `locales`.
    fn steps_json(locales: &[&str], steps: &[&str]) -> String {
        let list = steps
            .iter()
            .map(|s| format!(r#""{s}""#))
            .collect::<Vec<_>>()
            .join(",");
        let entries = locales
            .iter()
            .map(|l| format!(r#""{l}":[{list}]"#))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{entries}}}")
    }

    /// A minimal preset carrying `"mobileExport": { <guide> }`.
    fn preset_with_guide(guide: &str) -> String {
        format!(
            r#"{{"schemaVersion":1,"id":"x","bankName":"X","country":"CZ","matchHeaders":["Date","Amount"],
            "delimiter":";","encoding":"UTF-8","dateColumn":"Date","dateFormat":"%d.%m.%Y",
            "amountColumn":"Amount","descriptionColumns":["Note"],"mobileExport":{{{guide}}}}}"#
        )
    }

    /// A valid web guide with `extra` appended to its fields.
    fn web_guide(extra: &str) -> String {
        format!(
            r#""route":"web","format":"CSV","webUrl":"https://bank.example/","steps":{}{extra}"#,
            steps_json(
                &GUIDE_LOCALES,
                &["Sign in", "Export as CSV", "Share to Moony"]
            )
        )
    }

    #[test]
    fn a_mobile_export_guide_is_optional_and_parses() {
        let plain = parse_preset(&preset_with_guide(&web_guide(""))).expect("valid guide");
        let guide = plain.mobile_export.expect("guide");
        assert_eq!(guide.route, ExportRoute::Web);
        assert_eq!(guide.format, "CSV");
        assert_eq!(guide.web_url.as_deref(), Some("https://bank.example/"));
        assert_eq!(guide.app_url_scheme, None);
        assert_eq!(guide.steps["cs"].len(), 3);

        // A preset without the block is still a preset
        let without = parse_preset(
            r#"{"schemaVersion":1,"id":"x","bankName":"X","country":"CZ","matchHeaders":["Date","Amount"],
            "delimiter":";","encoding":"UTF-8","dateColumn":"Date","dateFormat":"%d.%m.%Y",
            "amountColumn":"Amount","descriptionColumns":["Note"]}"#,
        )
        .expect("a preset needs no guide");
        assert!(without.mobile_export.is_none());

        // The guide is a closed shape: a misspelt field is an error, not silently dropped
        let err = parse_preset(&preset_with_guide(&web_guide(r#","webUrls":[]"#))).unwrap_err();
        assert!(err.contains("webUrls"), "{err}");
    }

    #[test]
    fn a_mobile_export_guide_needs_all_ten_locales() {
        // Nine of ten: Japanese is missing
        let nine: Vec<&str> = GUIDE_LOCALES
            .iter()
            .copied()
            .filter(|l| *l != "ja")
            .collect();
        let guide = format!(
            r#""route":"web","format":"CSV","webUrl":"https://bank.example/","steps":{}"#,
            steps_json(&nine, &["Sign in", "Export as CSV", "Share to Moony"])
        );
        let err = parse_preset(&preset_with_guide(&guide)).unwrap_err();
        assert!(err.contains("'ja'"), "{err}");

        // A locale the app does not ship is a typo, not extra data
        let mut eleven = GUIDE_LOCALES.to_vec();
        eleven.push("cz");
        let guide = format!(
            r#""route":"web","format":"CSV","webUrl":"https://bank.example/","steps":{}"#,
            steps_json(&eleven, &["Sign in", "Export as CSV", "Share to Moony"])
        );
        let err = parse_preset(&preset_with_guide(&guide)).unwrap_err();
        assert!(err.contains("'cz'"), "{err}");

        // Two to four steps per locale, none of them blank
        for steps in [
            vec!["Only one"],
            vec!["One", "Two", "Three", "Four", "Five"],
            vec!["One", "  ", "Three"],
            vec!["One", "", "Three"],
        ] {
            let guide = format!(
                r#""route":"web","format":"CSV","webUrl":"https://bank.example/","steps":{}"#,
                steps_json(&GUIDE_LOCALES, &steps)
            );
            let err = parse_preset(&preset_with_guide(&guide)).unwrap_err();
            assert!(err.contains("steps"), "{steps:?}: {err}");
        }
        for steps in [vec!["One", "Two"], vec!["One", "Two", "Three", "Four"]] {
            let guide = format!(
                r#""route":"web","format":"CSV","webUrl":"https://bank.example/","steps":{}"#,
                steps_json(&GUIDE_LOCALES, &steps)
            );
            assert!(
                parse_preset(&preset_with_guide(&guide)).is_ok(),
                "{steps:?}"
            );
        }

        // The format the person picks is named
        let err = parse_preset(&preset_with_guide(
            &web_guide("").replace(r#""format":"CSV""#, r#""format":" ""#),
        ))
        .unwrap_err();
        assert!(err.contains("format"), "{err}");
    }

    #[test]
    fn a_web_route_needs_an_https_url() {
        let without = web_guide("").replace(r#""webUrl":"https://bank.example/","#, "");
        let err = parse_preset(&preset_with_guide(&without)).unwrap_err();
        assert!(err.contains("webUrl"), "{err}");

        for url in [
            "http://bank.example/",
            "bank.example",
            "javascript:alert(1)",
            "",
        ] {
            let guide = web_guide("").replace("https://bank.example/", url);
            let err = parse_preset(&preset_with_guide(&guide)).unwrap_err();
            assert!(err.contains("webUrl"), "{url}: {err}");
        }

        // An app route may leave it out, but one that is given is https too
        let app = format!(
            r#""route":"app","format":"CSV","steps":{}"#,
            steps_json(
                &GUIDE_LOCALES,
                &["Open the app", "Export as CSV", "Share to Moony"]
            )
        );
        assert!(parse_preset(&preset_with_guide(&app)).is_ok());
        let err = parse_preset(&preset_with_guide(&format!(
            r#"{app},"webUrl":"http://bank.example/""#
        )))
        .unwrap_err();
        assert!(err.contains("webUrl"), "{err}");
    }

    #[test]
    fn a_url_scheme_is_bare() {
        for scheme in ["fio", "mbank-app", "cz.fio.banking", "app2"] {
            let guide = web_guide(&format!(r#","appUrlScheme":"{scheme}""#));
            assert!(
                parse_preset(&preset_with_guide(&guide)).is_ok(),
                "{scheme} is a bare scheme"
            );
        }
        for scheme in [
            "fio://",
            "fio:",
            "fio://open",
            "fio banka",
            "1fio",
            "",
            "https",
            "javascript",
        ] {
            let guide = web_guide(&format!(r#","appUrlScheme":"{scheme}""#));
            let err = parse_preset(&preset_with_guide(&guide)).unwrap_err();
            assert!(err.contains("appUrlScheme"), "{scheme}: {err}");
        }
    }

    /// Every bank preset (not the generic UK layout) carries a guide for each locale; the guides
    /// hold what the bank-export research found (docs/plans/2026-10-16-mobile-app-bank-exports.md),
    /// so the app routes are exactly the banks whose own phone app that research documents
    /// (mBank, Wise, Revolut) and no preset invents an app scheme.
    #[test]
    fn every_bank_preset_carries_a_mobile_guide() {
        for preset in all_presets() {
            if preset.id == "uk-debit-credit" {
                assert!(
                    preset.mobile_export.is_none(),
                    "uk-debit-credit is a layout, not a bank"
                );
                continue;
            }
            if preset.id == "moneta" {
                // no guide until a device check: T1 found no CSV export
                assert!(
                    preset.mobile_export.is_none(),
                    "moneta has no guide until a device check"
                );
                continue;
            }
            let guide = preset
                .mobile_export
                .as_ref()
                .unwrap_or_else(|| panic!("{}: no mobileExport guide", preset.id));
            // Revolut's personal app offers Excel, which is a CSV for some people (T1)
            let format = if preset.id == "revolut" {
                "Excel (CSV)"
            } else {
                "CSV"
            };
            assert_eq!(guide.format, format, "{}", preset.id);
            assert_eq!(
                guide.app_url_scheme, None,
                "{}: no scheme is documented",
                preset.id
            );
            if matches!(preset.id.as_str(), "mbank" | "wise" | "revolut") {
                assert_eq!(guide.route, ExportRoute::App, "{}", preset.id);
                assert_eq!(
                    guide.web_url, None,
                    "{}: an app route has no web URL",
                    preset.id
                );
            } else {
                assert_eq!(guide.route, ExportRoute::Web, "{}", preset.id);
                assert!(
                    guide
                        .web_url
                        .as_deref()
                        .is_some_and(|url| url.starts_with("https://")),
                    "{}: a web route has an https URL",
                    preset.id
                );
            }
        }
    }

    #[test]
    fn detect_requires_every_match_header_and_ignores_case_and_accents() {
        // Not enough for any preset: nothing detected rather than a guess.
        assert!(detect_preset(&headers(&["Date", "Amount"])).is_none());
        assert!(detect_preset(&[]).is_none());

        // Shouted and accent-free spellings still detect the bank; extra
        // columns do not matter.
        let cs = headers(&[
            "DATUM ZAUCTOVANI",
            "CASTKA",
            "MENA",
            "NAZEV PROTIUCTU",
            "ZPRAVA PRO PRIJEMCE",
            "Něco navíc",
        ]);
        assert_eq!(
            detect_preset(&cs).map(|p| p.id.as_str()),
            Some("ceska-sporitelna")
        );

        // One matchHeader missing → no match.
        let incomplete = headers(&["Datum zaúčtování", "Částka", "Měna", "Název protiúčtu"]);
        assert!(detect_preset(&incomplete).is_none());
    }

    #[test]
    fn more_match_headers_wins_a_tie() {
        // Two synthetic presets matching the same headers: the more specific
        // one (more matchHeaders) is chosen by the fold.
        let a = parse_preset(
            r#"{"schemaVersion":1,"id":"a","bankName":"A","country":"CZ","matchHeaders":["Date","Amount"],
            "delimiter":",","encoding":"UTF-8","dateColumn":"Date","dateFormat":"%Y-%m-%d",
            "amountColumn":"Amount","descriptionColumns":["Note"]}"#,
        )
        .unwrap();
        let b = parse_preset(
            r#"{"schemaVersion":1,"id":"b","bankName":"B","country":"CZ","matchHeaders":["Date","Amount","Note"],
            "delimiter":",","encoding":"UTF-8","dateColumn":"Date","dateFormat":"%Y-%m-%d",
            "amountColumn":"Amount","descriptionColumns":["Note"]}"#,
        )
        .unwrap();
        let pick = |list: &[&CsvPreset]| {
            list.iter()
                .fold(None, |best: Option<&CsvPreset>, p| match best {
                    Some(x) if x.match_headers.len() >= p.match_headers.len() => Some(x),
                    _ => Some(*p),
                })
                .map(|p| p.id.clone())
        };
        assert_eq!(pick(&[&a, &b]).as_deref(), Some("b"));
        assert_eq!(pick(&[&b, &a]).as_deref(), Some("b"));
    }

    #[test]
    fn resolve_uses_the_files_own_spelling_and_drops_missing_optionals() {
        let preset = get_preset("revolut").unwrap();
        let h = headers(&[
            "type",
            "product",
            "started date",
            "COMPLETED DATE",
            "description",
            "amount",
            "fee",
            "currency",
            "state",
            "balance",
        ]);
        let r = resolve_preset(preset, &h).expect("resolves");
        assert_eq!(r.date, "COMPLETED DATE");
        assert_eq!(r.amount.as_deref(), Some("amount"));
        assert_eq!(r.currency.as_deref(), Some("currency"));
        assert_eq!(r.balance.as_deref(), Some("balance"));
        assert_eq!(r.description, vec!["description"]);
        assert_eq!(
            r.row_filter.as_ref().map(|f| f.column.as_str()),
            Some("state")
        );
        assert_eq!(
            r.row_filter.as_ref().map(|f| f.equals.as_str()),
            Some("COMPLETED")
        );

        // Without the date column the preset does not describe the file.
        let no_date = headers(&["description", "amount", "state"]);
        assert!(resolve_preset(preset, &no_date).is_none());
    }

    // ---- the contribution gate ----

    /// A CSV that exercises every column a preset names, in the preset's own
    /// delimiter and date format. Used for presets without a real fixture.
    fn synthetic_csv(p: &CsvPreset) -> String {
        let mut columns: Vec<String> = p.match_headers.clone();
        let named = p
            .description_columns
            .iter()
            .chain([&p.date_column])
            .chain(p.amount_column.iter())
            .chain(p.debit_column.iter())
            .chain(p.credit_column.iter())
            .chain(p.counterparty_column.iter())
            .chain(p.counterparty_iban_column.iter())
            .chain(p.currency_column.iter())
            .chain(p.balance_column.iter())
            .chain(p.transaction_id_column.iter());
        for column in named {
            if !columns
                .iter()
                .any(|c| normalize_header(c) == normalize_header(column))
            {
                columns.push(column.clone());
            }
        }

        let is = |column: &str, wanted: &Option<String>| {
            wanted
                .as_deref()
                .is_some_and(|w| normalize_header(w) == normalize_header(column))
        };
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 2)
            .and_then(|d| d.and_hms_opt(10, 0, 0))
            .expect("valid date")
            .format(&p.date_format)
            .to_string();
        let cells: Vec<String> = columns
            .iter()
            .map(|column| {
                let n = normalize_header(column);
                if n == normalize_header(&p.date_column) {
                    date.clone()
                } else if is(column, &p.amount_column) {
                    "-12.50".into()
                } else if is(column, &p.debit_column) {
                    "12.50".into()
                } else if is(column, &p.credit_column) {
                    String::new()
                } else if is(column, &p.currency_column) {
                    "EUR".into()
                } else if is(column, &p.balance_column) {
                    "100.00".into()
                } else if is(column, &p.transaction_id_column) {
                    "TX-1".into()
                } else if is(column, &p.counterparty_iban_column) {
                    "DE89370400440532013000".into()
                } else if is(column, &p.counterparty_column) {
                    "Shop".into()
                } else if let Some(f) = p
                    .row_filter
                    .as_ref()
                    .filter(|f| normalize_header(&f.column) == n)
                {
                    f.equals.clone()
                } else {
                    "some text".into()
                }
            })
            .collect();
        let d = &p.delimiter;
        format!("{}\n{}\n", columns.join(d), cells.join(d))
    }

    /// The gate every new preset must pass: it parses, is internally
    /// consistent, is detected from its own headers (and not shadowed by another
    /// preset), and imports its sample — the real fixture when it names one,
    /// else a synthetic file built from its own columns — without a single
    /// error.
    #[test]
    fn all_presets_are_valid() {
        use crate::services::csv_import::decode::{
            detect_csv_delimiter, detect_header_row, read_headers,
        };
        use crate::services::csv_import::test_support::*;
        use crate::services::csv_import::{
            decode_for_import, import_transactions, inspect_csv, InspectOptions,
        };

        for source in PRESET_SOURCES {
            let preset = parse_preset(source).unwrap_or_else(|e| panic!("{e}"));
            let id = preset.id.as_str();

            let bytes: Vec<u8> = match &preset.sample_fixture {
                Some(name) => fixture_bytes(name),
                None => synthetic_csv(&preset).into_bytes(),
            };
            let delimiter = preset.delimiter.chars().next().expect("delimiter");
            let text = decode_for_import(&bytes, None, None).text;
            assert_eq!(
                detect_csv_delimiter(&text),
                delimiter,
                "{id}: the preset's delimiter is not what the sample uses"
            );

            let header_row = detect_header_row(&text, delimiter);
            let file_headers = read_headers(&text, delimiter, header_row);
            let detected = detect_preset(&file_headers)
                .unwrap_or_else(|| panic!("{id}: not detected from its own sample headers"));
            assert_eq!(
                detected.id, preset.id,
                "{id}: shadowed by '{}' (it matches the same headers with more matchHeaders)",
                detected.id
            );

            let resolved = resolve_preset(&preset, &file_headers)
                .unwrap_or_else(|| panic!("{id}: columns do not resolve against its sample"));
            let mut config = resolved.to_config(&preset.delimiter, &preset.date_format);
            config.header_row = Some(header_row);

            let mut conn = setup_import_db();
            let result =
                import_transactions(&mut conn, "acc-1", "sample.csv", &text, &config, None)
                    .unwrap_or_else(|e| panic!("{id}: sample import failed: {e}"));
            assert_eq!(
                result.error_count, 0,
                "{id}: sample has errors: {:?}",
                result.errors
            );
            assert!(
                result.imported_count >= 1,
                "{id}: nothing imported from the sample"
            );
            assert!(
                preset.balance_column.is_none() || result.last_balance.is_some(),
                "{id}: balanceColumn set but no balance read"
            );

            // And the inspection (what the dialog sees) agrees.
            let inspection = inspect_csv(&bytes, &InspectOptions::default())
                .unwrap_or_else(|e| panic!("{id}: inspect failed: {e}"));
            assert_eq!(inspection.detected_preset_id.as_deref(), Some(id), "{id}");
            assert!(
                !inspection.date_format_ambiguous,
                "{id}: preset date format not confirmed"
            );
        }
    }

    #[test]
    fn every_bank_fixture_belongs_to_a_preset_or_is_a_generic_case() {
        // New statement fixtures must either back a preset or be listed here
        // as deliberately preset-less.
        const GENERIC: [&str; 5] = [
            "generic-iso.csv",
            "generic-iso-overlap.csv",
            "edge-cases.csv",
            // Files no preset reads, for the column inference (`csv_import::infer`).
            "unknown-unicredit-like.csv",
            "unknown-opaque-headers.csv",
        ];
        let backed: HashSet<&str> = all_presets()
            .iter()
            .filter_map(|p| p.sample_fixture.as_deref())
            .collect();
        let mut seen = 0;
        for entry in std::fs::read_dir(crate::services::csv_import::test_support::FIXTURE_DIR)
            .expect("fixture dir")
        {
            let name = entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string();
            if !name.ends_with(".csv") || name.starts_with("stocks-") {
                continue;
            }
            seen += 1;
            assert!(
                backed.contains(name.as_str()) || GENERIC.contains(&name.as_str()),
                "fixture {name} is neither a preset sample nor listed as generic"
            );
        }
        assert_eq!(seen, 13, "the 13 bank statement fixtures");
    }
}
