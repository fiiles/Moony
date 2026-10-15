//! CAMT.053 (ISO 20022 bank-to-customer statement) parser, field by field as in the
//! design spec §6.1. Elements are matched by local name, so every namespace version
//! (`camt.053.001.02` to `.13`) parses the same way. One `Stmt` is one [`Statement`].

use chrono::NaiveDate;

use crate::error::{AppError, Result};
use crate::services::csv_import::DeclaredBalance;
use crate::services::dedup::canonical_amount;
use crate::services::financial_semantics::valid_iban;

use super::decimal::Dec;
use super::model::{join_description, malformed, utc_day, AccountRef, Entry, Statement};
use super::reject_doctype;
use super::xml_tree::{parse_xml, Node};

/// Row message key of an entry that is neither booked nor pending (`INFO`, `FUTR`, ...).
const NOT_BOOKED: &str = "statementImport.notBooked";

/// Parses a CAMT.053 document into its statements, in file order.
pub fn parse(text: &str) -> Result<Vec<Statement>> {
    reject_doctype(text)?;
    let root = parse_xml(text)?;
    let statements = root
        .at(&["BkToCstmrStmt"])
        .into_iter()
        .flat_map(|group| group.children("Stmt"))
        .map(statement)
        .collect::<Result<Vec<_>>>()?;
    if statements.is_empty() {
        return Err(AppError::Validation(
            "validation.statementNoStatements".into(),
        ));
    }
    Ok(statements)
}

fn statement(stmt: &Node) -> Result<Statement> {
    let closing = balance(stmt, &["CLBD"]);
    let currency = stmt
        .text_at(&["Acct", "Ccy"])
        .map(str::to_uppercase)
        .or_else(|| closing.as_ref().map(|b| b.currency.clone()));
    let period = stmt
        .text_at(&["FrToDt", "FrDtTm"])
        .and_then(iso_day)
        .zip(stmt.text_at(&["FrToDt", "ToDtTm"]).and_then(iso_day));
    Ok(Statement {
        account: account_ref(stmt),
        currency,
        period,
        opening: balance(stmt, &["OPBD", "PRCD"]),
        closing,
        sequence: stmt
            .text_at(&["ElctrncSeqNb"])
            .or_else(|| stmt.text_at(&["LglSeqNb"]))
            .map(str::to_string),
        entries: entries(stmt)?,
    })
}

fn account_ref(stmt: &Node) -> AccountRef {
    let id = stmt.at(&["Acct", "Id"]);
    let identifier = id
        .and_then(|id| {
            id.text_at(&["IBAN"])
                .or_else(|| id.text_at(&["Othr", "Id"]))
        })
        .map(str::to_string);
    let bank_code = stmt
        .at(&["Acct", "Svcr", "FinInstnId"])
        .and_then(institution_code)
        .or_else(|| {
            let scheme = id?.at(&["Othr", "SchmeNm"])?;
            scheme
                .text_at(&["Cd"])
                .or_else(|| scheme.text_at(&["Prtry"]))
                .map(str::to_string)
        });
    AccountRef {
        identifier,
        bank_code,
        name: None,
    }
}

/// The first `Bal` whose type code is one of `codes` (earlier codes win), with a
/// readable amount, currency, day and direction. `DBIT` makes the amount negative; a
/// balance whose `CdtDbtInd` is not `CRDT` or `DBIT` is not read (no declared balance).
fn balance(stmt: &Node, codes: &[&str]) -> Option<DeclaredBalance> {
    codes.iter().find_map(|code| {
        stmt.children("Bal")
            .filter(|bal| {
                bal.text_at(&["Tp", "CdOrPrtry", "Cd"])
                    .is_some_and(|found| found.eq_ignore_ascii_case(code))
            })
            .find_map(|bal| declared_balance(stmt, bal))
    })
}

fn declared_balance(stmt: &Node, bal: &Node) -> Option<DeclaredBalance> {
    let amt = bal.child("Amt")?;
    let value = amount(&amt.text)?;
    let direction = bal.text_at(&["CdtDbtInd"])?;
    let signed = if direction.eq_ignore_ascii_case("DBIT") {
        value.neg()
    } else if direction.eq_ignore_ascii_case("CRDT") {
        value
    } else {
        return None;
    };
    Some(DeclaredBalance {
        amount: signed.to_text(),
        currency: amt
            .attr("Ccy")
            .or_else(|| stmt.text_at(&["Acct", "Ccy"]))?
            .to_uppercase(),
        as_of: date_at(bal, "Dt")?,
    })
}

fn entries(stmt: &Node) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for (index, ntry) in stmt.children("Ntry").enumerate() {
        let parsed = ntry_entries(ntry).ok_or_else(|| malformed(&format!("Ntry {}", index + 1)))?;
        entries.extend(parsed);
    }
    Ok(entries)
}

/// The entries of one `Ntry`: one per `TxDtls` of a batch that adds up, else one. `None`
/// when the amount, direction or booking day cannot be read.
fn ntry_entries(ntry: &Node) -> Option<Vec<Entry>> {
    let amt = ntry.child("Amt")?;
    let total = amount(&amt.text)?;
    let is_credit = match ntry.text_at(&["CdtDbtInd"])? {
        "CRDT" => true,
        "DBIT" => false,
        _ => return None,
    };
    let status = status(ntry);
    let base = Entry {
        booking_date: date_at(ntry, "BookgDt").or_else(|| date_at(ntry, "ValDt"))?,
        value_date: date_at(ntry, "ValDt"),
        is_credit,
        currency: amt.attr("Ccy").map(str::to_string),
        pending: status == "PDNG",
        skip_reason: (status != "BOOK" && status != "PDNG").then_some(NOT_BOOKED),
        ..Entry::default()
    };
    let entry_id = ntry
        .text_at(&["AcctSvcrRef"])
        .or_else(|| ntry.text_at(&["NtryRef"]));
    let txs: Vec<&Node> = ntry
        .children("NtryDtls")
        .flat_map(|details| details.children("TxDtls"))
        .collect();

    if let Some(parts) = split_amounts(&txs, total, amt.attr("Ccy")) {
        return Some(
            txs.iter()
                .zip(parts)
                .enumerate()
                .map(|(index, (tx, part))| {
                    let bank_id = tx
                        .text_at(&["Refs", "AcctSvcrRef"])
                        .map(str::to_string)
                        .or_else(|| entry_id.map(|id| format!("{id}/{}", index + 1)));
                    entry_with_details(&base, ntry, tx, bank_id, part)
                })
                .collect(),
        );
    }
    // Without TxDtls the entry itself carries the parties and remittance (some banks).
    let tx = txs.first().copied().unwrap_or(ntry);
    let tx_id = tx.text_at(&["Refs", "AcctSvcrRef"]);
    // One TxDtls refers to the whole entry. Of several that did not split, the first
    // one's ref names only a part of the batch, so the entry's own id comes first.
    let bank_id = if txs.len() >= 2 {
        entry_id.or(tx_id)
    } else {
        tx_id.or(entry_id)
    };
    Some(vec![entry_with_details(
        &base,
        ntry,
        tx,
        bank_id.map(str::to_string),
        total.to_text(),
    )])
}

/// The amount of every `TxDtls` of a batch, when there are at least two, each has one
/// in the entry's currency and together they add up to the entry amount.
fn split_amounts(txs: &[&Node], total: Dec, currency: Option<&str>) -> Option<Vec<String>> {
    if txs.len() < 2 {
        return None;
    }
    let mut sum = Dec::zero();
    let mut parts = Vec::with_capacity(txs.len());
    for tx in txs {
        let amt = tx
            .at(&["AmtDtls", "TxAmt", "Amt"])
            .or_else(|| tx.child("Amt"))?;
        let part_currency = amt.attr("Ccy");
        if part_currency.is_some() && part_currency != currency {
            return None;
        }
        let part = amount(&amt.text)?;
        sum = sum.add(part)?;
        parts.push(part.to_text());
    }
    (sum == total).then_some(parts)
}

fn entry_with_details(
    base: &Entry,
    ntry: &Node,
    tx: &Node,
    bank_id: Option<String>,
    amount: String,
) -> Entry {
    let party = party(tx, base.is_credit);
    Entry {
        amount,
        bank_id,
        description: description(tx)
            .or_else(|| ntry.text_at(&["AddtlNtryInf"]).map(str::to_string)),
        counterparty_name: party.name,
        counterparty_account: party.account,
        counterparty_bank_code: party.bank_code,
        counterparty_country: party.country,
        ..base.clone()
    }
}

/// `Sts/Cd` (newer versions) or the text of `Sts` (older ones), upper-cased.
fn status(ntry: &Node) -> String {
    ntry.text_at(&["Sts", "Cd"])
        .or_else(|| ntry.text_at(&["Sts"]))
        .unwrap_or_default()
        .to_uppercase()
}

#[derive(Default)]
struct Party {
    name: Option<String>,
    account: Option<String>,
    bank_code: Option<String>,
    country: Option<String>,
}

/// The other side of the movement: the debtor of a credit, the creditor of a debit.
fn party(tx: &Node, credit: bool) -> Party {
    let (who, who_account, who_agent) = if credit {
        ("Dbtr", "DbtrAcct", "DbtrAgt")
    } else {
        ("Cdtr", "CdtrAcct", "CdtrAgt")
    };
    let parties = tx.child("RltdPties");
    let name = parties.and_then(|p| {
        p.text_at(&[who, "Nm"])
            .or_else(|| p.text_at(&[who, "Pty", "Nm"]))
    });
    let account = parties
        .and_then(|p| p.at(&[who_account, "Id"]))
        .and_then(|id| {
            id.text_at(&["IBAN"])
                .or_else(|| id.text_at(&["Othr", "Id"]))
        });
    let country = account.and_then(iban_country).or_else(|| {
        let p = parties?;
        p.text_at(&[who, "PstlAdr", "Ctry"])
            .or_else(|| p.text_at(&[who, "Pty", "PstlAdr", "Ctry"]))
            .map(str::to_string)
    });
    Party {
        name: name.map(str::to_string),
        account: account.map(str::to_string),
        bank_code: tx
            .at(&["RltdAgts", who_agent, "FinInstnId"])
            .and_then(institution_code),
        country,
    }
}

/// `BICFI`, else `BIC`, else the clearing-system member id.
fn institution_code(institution: &Node) -> Option<String> {
    institution
        .text_at(&["BICFI"])
        .or_else(|| institution.text_at(&["BIC"]))
        .or_else(|| institution.text_at(&["ClrSysMmbId", "MmbId"]))
        .map(str::to_string)
}

/// The first two letters of an account that is a valid IBAN.
fn iban_country(account: &str) -> Option<String> {
    let compact: String = account
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    valid_iban(&compact).then(|| compact[..2].to_string())
}

/// Every `RmtInf/Ustrd`, then every structured creditor reference, then `AddtlTxInf`.
fn description(tx: &Node) -> Option<String> {
    let unstructured = tx
        .children("RmtInf")
        .flat_map(|remittance| remittance.children("Ustrd"))
        .map(|node| node.text.as_str());
    let structured = tx
        .children("RmtInf")
        .flat_map(|remittance| remittance.children("Strd"))
        .filter_map(|node| node.text_at(&["CdtrRefInf", "Ref"]));
    let additional = tx.children("AddtlTxInf").map(|node| node.text.as_str());
    join_description(unstructured.chain(structured).chain(additional))
}

/// A non-negative amount, exact. `None` unless the text is a plain decimal: for blank,
/// sign-only or digit-less text `canonical_amount` answers "0", which must not pass.
fn amount(text: &str) -> Option<Dec> {
    let text = text.trim();
    Dec::parse(text)?;
    let canonical = canonical_amount(text);
    if canonical.starts_with('-') {
        return None;
    }
    Dec::parse(&canonical)
}

/// The day of `<name><Dt>` or `<name><DtTm>`.
fn date_at(node: &Node, name: &str) -> Option<i64> {
    node.text_at(&[name, "Dt"])
        .or_else(|| node.text_at(&[name, "DtTm"]))
        .and_then(iso_day)
}

/// The calendar day as written (the first ten characters), never shifted by a time zone.
fn iso_day(text: &str) -> Option<i64> {
    let day = text.trim().get(..10)?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().map(utc_day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::statement_import::model::utc_day;
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> i64 {
        utc_day(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }
    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/statements/camt053/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn basic_statement() {
        let s = &parse(&fixture("basic.xml")).unwrap()[0];
        assert_eq!(
            s.account.identifier.as_deref(),
            Some("CZ6508000000192000145399")
        );
        assert_eq!(s.currency.as_deref(), Some("CZK"));
        assert_eq!(s.period, Some((day(2026, 9, 1), day(2026, 9, 30))));
        assert_eq!(
            s.opening.as_ref().map(|b| (b.amount.as_str(), b.as_of)),
            Some(("1000", day(2026, 8, 31)))
        );
        assert_eq!(
            s.closing.as_ref().map(|b| b.amount.as_str()),
            Some("1250.5")
        );
        assert_eq!(s.sequence.as_deref(), Some("9"));
        let first = &s.entries[0];
        assert_eq!(
            (
                first.amount.as_str(),
                first.is_credit,
                first.bank_id.as_deref()
            ),
            ("500.5", true, Some("REF-1"))
        );
        assert_eq!(
            first.counterparty_name.as_deref(),
            Some("Acme & Söhne s.r.o.")
        );
        assert_eq!(
            first.counterparty_account.as_deref(),
            Some("CZ5508000000001234567899")
        );
        assert_eq!(first.counterparty_bank_code.as_deref(), Some("GIBACZPX"));
        assert_eq!(first.counterparty_country.as_deref(), Some("CZ"));
        assert_eq!(first.description.as_deref(), Some("Faktura 2026-17"));
        assert_eq!(first.value_date, Some(day(2026, 9, 5)));
        let second = &s.entries[1];
        assert_eq!(second.booking_date, day(2026, 9, 20)); // DtTm with +02:00, never shifted
        assert_eq!(second.counterparty_name.as_deref(), Some("Lidl ČR"));
        assert_eq!(second.description.as_deref(), Some("Platba kartou"));
    }

    #[test]
    fn batch_split_pending_info_and_newer_version() {
        let s = &parse(&fixture("batch-and-pending.xml")).unwrap()[0];
        assert_eq!(s.opening.as_ref().map(|b| b.amount.as_str()), Some("0"));
        let ids: Vec<_> = s.entries.iter().map(|e| e.bank_id.as_deref()).collect();
        assert_eq!(
            ids,
            vec![Some("B-1-a"), Some("B-1/2"), Some("P-1"), None, Some("N-4")]
        );
        assert_eq!(s.entries[0].amount, "60");
        assert_eq!(s.entries[0].counterparty_name.as_deref(), Some("Jan Novák"));
        assert!(s.entries[2].pending);
        assert_eq!(s.entries[3].skip_reason, Some("statementImport.notBooked"));
        assert_eq!(s.entries[4].counterparty_name.as_deref(), Some("Shop"));
        assert_eq!(s.entries[4].description.as_deref(), Some("Card 1234"));
    }

    #[test]
    fn batch_that_does_not_add_up_stays_one_entry() {
        let text = fixture("batch-and-pending.xml").replace(
            "<Amt Ccy=\"CZK\">40.00</Amt>",
            "<Amt Ccy=\"CZK\">41.00</Amt>",
        );
        let s = &parse(&text).unwrap()[0];
        assert_eq!(s.entries[0].amount, "100");
        assert_eq!(s.entries[0].bank_id.as_deref(), Some("B-1"));
        assert_eq!(s.entries[0].counterparty_name.as_deref(), Some("Jan Novák"));
    }

    #[test]
    fn debit_balance_is_negative_and_doctype_refused() {
        let text = fixture("basic.xml").replacen(
            "<CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-08-31",
            "<CdtDbtInd>DBIT</CdtDbtInd><Dt><Dt>2026-08-31",
            1,
        );
        assert_eq!(
            parse(&text).unwrap()[0].opening.as_ref().unwrap().amount,
            "-1000"
        );
        let doctype = fixture("basic.xml").replacen("?>", "?><!DOCTYPE Document>", 1);
        assert!(parse(&doctype)
            .unwrap_err()
            .to_string()
            .contains("validation.statementDoctype"));
    }

    #[test]
    fn malformed_xml_is_a_validation_error() {
        assert!(parse("<Document><Stmt>")
            .unwrap_err()
            .to_string()
            .contains("validation.statementMalformed"));
    }

    /// A prefixed camt.053.001.13 document with the optional parts left out or written
    /// the older way: no `Acct/Ccy`, no period, a domestic account, entry-level parties.
    const SPARSE: &str = r#"<?xml version="1.0"?>
<c:Document xmlns:c="urn:iso:std:iso:20022:tech:xsd:camt.053.001.13">
 <c:BkToCstmrStmt>
  <c:Stmt>
   <c:LglSeqNb>42</c:LglSeqNb>
   <c:Acct><c:Id><c:Othr><c:Id>19-2000145399</c:Id></c:Othr></c:Id><c:Svcr><c:FinInstnId><c:ClrSysMmbId><c:MmbId>0800</c:MmbId></c:ClrSysMmbId></c:FinInstnId></c:Svcr></c:Acct>
   <c:Bal><c:Tp><c:CdOrPrtry><c:Cd>PRCD</c:Cd></c:CdOrPrtry></c:Tp><c:Amt Ccy="CZK">5.00</c:Amt><c:CdtDbtInd>CRDT</c:CdtDbtInd><c:Dt><c:Dt>2026-08-30</c:Dt></c:Dt></c:Bal>
   <c:Bal><c:Tp><c:CdOrPrtry><c:Cd>OPBD</c:Cd></c:CdOrPrtry></c:Tp><c:Amt Ccy="CZK">7.00</c:Amt><c:CdtDbtInd>CRDT</c:CdtDbtInd><c:Dt><c:Dt>2026-08-31</c:Dt></c:Dt></c:Bal>
   <c:Bal><c:Tp><c:CdOrPrtry><c:Cd>CLBD</c:Cd></c:CdOrPrtry></c:Tp><c:Amt Ccy="CZK">9.50</c:Amt><c:CdtDbtInd>CRDT</c:CdtDbtInd><c:Dt><c:DtTm>2026-09-30T00:00:00</c:DtTm></c:Dt></c:Bal>
   <c:Ntry>
    <c:NtryRef>NR-1</c:NtryRef><c:Amt Ccy="CZK">12.30</c:Amt><c:CdtDbtInd>DBIT</c:CdtDbtInd><c:Sts>book</c:Sts>
    <c:ValDt><c:DtTm>2026-09-02T10:00:00Z</c:DtTm></c:ValDt>
    <c:RltdPties><c:Cdtr><c:Nm>Kiosk</c:Nm><c:PstlAdr><c:Ctry>SK</c:Ctry></c:PstlAdr></c:Cdtr><c:CdtrAcct><c:Id><c:Othr><c:Id>123456</c:Id></c:Othr></c:Id></c:CdtrAcct></c:RltdPties>
    <c:RltdAgts><c:CdtrAgt><c:FinInstnId><c:ClrSysMmbId><c:MmbId>0300</c:MmbId></c:ClrSysMmbId></c:FinInstnId></c:CdtrAgt></c:RltdAgts>
    <c:RmtInf><c:Strd><c:CdtrRefInf><c:Ref>RF18</c:Ref></c:CdtrRefInf></c:Strd><c:Ustrd>First</c:Ustrd><c:Ustrd>Second</c:Ustrd></c:RmtInf>
    <c:AddtlTxInf>Extra</c:AddtlTxInf>
   </c:Ntry>
   <c:Ntry>
    <c:Amt Ccy="CZK">3.00</c:Amt><c:CdtDbtInd>CRDT</c:CdtDbtInd><c:Sts><c:Cd>FUTR</c:Cd></c:Sts>
    <c:BookgDt><c:Dt>2026-10-02</c:Dt></c:BookgDt>
   </c:Ntry>
  </c:Stmt>
 </c:BkToCstmrStmt>
</c:Document>"#;

    #[test]
    fn prefixed_sparse_and_older_style_documents() {
        let s = &parse(SPARSE).unwrap()[0];
        assert_eq!(s.account.identifier.as_deref(), Some("19-2000145399"));
        assert_eq!(s.account.bank_code.as_deref(), Some("0800"));
        assert_eq!(s.sequence.as_deref(), Some("42"));
        assert_eq!(s.currency.as_deref(), Some("CZK")); // from the CLBD balance
        assert_eq!(s.period, None);
        // OPBD wins over PRCD, whichever comes first in the file.
        let opening = s.opening.as_ref().unwrap();
        assert_eq!(
            (opening.amount.as_str(), opening.as_of),
            ("7", day(2026, 8, 31))
        );
        assert_eq!(s.closing.as_ref().unwrap().as_of, day(2026, 9, 30));

        let e = &s.entries[0];
        assert_eq!(
            (e.amount.as_str(), e.is_credit, e.pending),
            ("12.3", false, false)
        );
        assert_eq!(e.skip_reason, None);
        assert_eq!(e.currency.as_deref(), Some("CZK"));
        assert_eq!(e.bank_id.as_deref(), Some("NR-1"));
        assert_eq!(e.booking_date, day(2026, 9, 2)); // no BookgDt: the value date
        assert_eq!(e.value_date, Some(day(2026, 9, 2)));
        assert_eq!(e.counterparty_name.as_deref(), Some("Kiosk"));
        assert_eq!(e.counterparty_account.as_deref(), Some("123456"));
        assert_eq!(e.counterparty_bank_code.as_deref(), Some("0300"));
        assert_eq!(e.counterparty_country.as_deref(), Some("SK")); // PstlAdr, not an IBAN
        assert_eq!(
            e.description.as_deref(),
            Some("First | Second | RF18 | Extra")
        );
        assert_eq!(s.entries[1].skip_reason, Some("statementImport.notBooked"));
        assert!(!s.entries[1].pending);
    }

    #[test]
    fn several_statements_come_out_in_file_order() {
        let text = fixture("basic.xml").replace(
            "</Stmt>",
            "</Stmt><Stmt><Id>S9</Id><Acct><Id><IBAN>CZ5508000000001234567899</IBAN></Id></Acct></Stmt>",
        );
        let statements = parse(&text).unwrap();
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].entries.len(), 2);
        assert_eq!(
            statements[1].account.identifier.as_deref(),
            Some("CZ5508000000001234567899")
        );
        assert!(statements[1].entries.is_empty());
        assert_eq!(
            (
                statements[1].currency.as_deref(),
                statements[1].opening.as_ref()
            ),
            (None, None)
        );
    }

    #[test]
    fn a_file_without_statements_is_refused() {
        for text in [
            "<Document><BkToCstmrStmt/></Document>",
            "<Document><BkToCstmrStmt><GrpHdr/></BkToCstmrStmt></Document>",
            "<Document/>",
        ] {
            let error = parse(text).unwrap_err();
            assert_eq!(
                error.keyed().expect("keyed").0,
                "validation.statementNoStatements",
                "{text}"
            );
        }
    }

    fn malformed_detail(text: &str) -> String {
        let (key, params) = parse(text).unwrap_err().keyed().expect("keyed");
        assert_eq!(key, "validation.statementMalformed");
        params["detail"].as_str().expect("detail").to_string()
    }

    #[test]
    fn an_entry_that_cannot_be_read_names_its_position() {
        let basic = fixture("basic.xml");
        let no_booking_day = basic.replace(
            "<BookgDt><DtTm>2026-09-20T23:30:00+02:00</DtTm></BookgDt>",
            "",
        );
        assert_eq!(malformed_detail(&no_booking_day), "Ntry 2");
        let bad_amount = basic.replace(
            "<Amt Ccy=\"CZK\">500.50</Amt>",
            "<Amt Ccy=\"CZK\">abc</Amt>",
        );
        assert_eq!(malformed_detail(&bad_amount), "Ntry 1");
        let negative = basic.replace(
            "<Amt Ccy=\"CZK\">500.50</Amt>",
            "<Amt Ccy=\"CZK\">-500.50</Amt>",
        );
        assert_eq!(malformed_detail(&negative), "Ntry 1");
        let no_direction = basic.replace(
            "</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>",
            "</Amt><CdtDbtInd>XXXX</CdtDbtInd><Sts>",
        );
        assert_eq!(malformed_detail(&no_direction), "Ntry 1");
    }

    /// What `canonical_amount` alone would read as 0, plus a digit-less exponent.
    const NOT_AMOUNTS: [&str; 6] = ["", "  ", "-", "+", ".", "e5"];

    fn amt(text: &str) -> String {
        if text.is_empty() {
            "<Amt Ccy=\"CZK\"/>".to_string()
        } else {
            format!("<Amt Ccy=\"CZK\">{text}</Amt>")
        }
    }

    #[test]
    fn blank_or_sign_only_entry_amounts_are_malformed_not_zero() {
        let basic = fixture("basic.xml");
        for text in NOT_AMOUNTS {
            let first = basic.replace("<Amt Ccy=\"CZK\">500.50</Amt>", &amt(text));
            assert_eq!(malformed_detail(&first), "Ntry 1", "{text:?}");
            let second = basic.replace("<Amt Ccy=\"CZK\">250.00</Amt>", &amt(text));
            assert_eq!(malformed_detail(&second), "Ntry 2", "{text:?}");
        }
    }

    #[test]
    fn blank_or_sign_only_balance_amounts_are_no_balance() {
        let basic = fixture("basic.xml");
        for text in NOT_AMOUNTS {
            let opening = basic.replace("<Amt Ccy=\"CZK\">1000.00</Amt>", &amt(text));
            let s = &parse(&opening).unwrap()[0];
            assert_eq!(s.opening, None, "{text:?}");
            assert!(s.closing.is_some(), "{text:?}");
            let closing = basic.replace("<Amt Ccy=\"CZK\">1250.50</Amt>", &amt(text));
            let s = &parse(&closing).unwrap()[0];
            assert_eq!(s.closing, None, "{text:?}");
            assert_eq!(s.currency.as_deref(), Some("CZK"), "{text:?}");
            assert!(s.opening.is_some(), "{text:?}");
        }
    }

    #[test]
    fn a_balance_without_a_readable_direction_is_no_balance() {
        let basic = fixture("basic.xml");
        let opening = "<Amt Ccy=\"CZK\">1000.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>";
        let closing = "<Amt Ccy=\"CZK\">1250.50</Amt><CdtDbtInd>CRDT</CdtDbtInd>";
        assert!(basic.contains(opening) && basic.contains(closing));
        for indicator in [
            "",
            "<CdtDbtInd/>",
            "<CdtDbtInd>XXXX</CdtDbtInd>",
            "<CdtDbtInd>CR</CdtDbtInd>",
        ] {
            let text = basic.replace(
                closing,
                &closing.replace("<CdtDbtInd>CRDT</CdtDbtInd>", indicator),
            );
            let s = &parse(&text).unwrap()[0];
            assert_eq!(s.closing, None, "closing with {indicator:?}");
            assert!(s.opening.is_some(), "opening with {indicator:?}");
            assert_eq!(s.currency.as_deref(), Some("CZK"));

            let text = basic.replace(
                opening,
                &opening.replace("<CdtDbtInd>CRDT</CdtDbtInd>", indicator),
            );
            let s = &parse(&text).unwrap()[0];
            assert_eq!(s.opening, None, "opening with {indicator:?}");
            assert!(s.closing.is_some(), "closing with {indicator:?}");
        }
    }

    #[test]
    fn the_direction_of_a_balance_is_trimmed_and_case_insensitive() {
        let basic = fixture("basic.xml");
        let text = basic
            .replacen(
                "<CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-08-31",
                "<CdtDbtInd> dbit </CdtDbtInd><Dt><Dt>2026-08-31",
                1,
            )
            .replacen(
                "<CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-09-30",
                "<CdtDbtInd>crdt</CdtDbtInd><Dt><Dt>2026-09-30",
                1,
            );
        let s = &parse(&text).unwrap()[0];
        assert_eq!(s.opening.as_ref().unwrap().amount, "-1000");
        assert_eq!(s.closing.as_ref().unwrap().amount, "1250.5");
    }

    #[test]
    fn currencies_of_the_statement_and_its_balances_are_upper_case() {
        let text = fixture("basic.xml").replace("Ccy=\"CZK\"", "Ccy=\"czk\"");
        let text = text.replace("<Ccy>CZK</Ccy>", "<Ccy>czk</Ccy>");
        let s = &parse(&text).unwrap()[0];
        assert_eq!(s.currency.as_deref(), Some("CZK"));
        assert_eq!(s.opening.as_ref().unwrap().currency, "CZK");
        assert_eq!(s.closing.as_ref().unwrap().currency, "CZK");

        // Without `Acct/Ccy` the statement takes the closing balance's currency.
        let no_acct = fixture("basic.xml")
            .replace("<Ccy>CZK</Ccy>", "")
            .replace("Ccy=\"CZK\"", "Ccy=\"czk\"");
        let s = &parse(&no_acct).unwrap()[0];
        assert_eq!(s.currency.as_deref(), Some("CZK"));
        assert_eq!(s.closing.as_ref().unwrap().currency, "CZK");

        // A balance without its own currency takes the (upper-cased) account currency.
        let inherited = fixture("basic.xml")
            .replace("<Ccy>CZK</Ccy>", "<Ccy>czk</Ccy>")
            .replace("<Amt Ccy=\"CZK\">1000.00</Amt>", "<Amt>1000.00</Amt>");
        let s = &parse(&inherited).unwrap()[0];
        assert_eq!(s.opening.as_ref().unwrap().currency, "CZK");
    }

    #[test]
    fn a_batch_in_another_currency_stays_one_entry() {
        let text = fixture("batch-and-pending.xml").replace(
            "<Amt Ccy=\"CZK\">40.00</Amt>",
            "<Amt Ccy=\"EUR\">40.00</Amt>",
        );
        let s = &parse(&text).unwrap()[0];
        assert_eq!(s.entries.len(), 4);
        assert_eq!(s.entries[0].amount, "100");
        assert_eq!(s.entries[0].bank_id.as_deref(), Some("B-1"));
    }
}
