//! OFX 1.x (SGML) and 2.x (XML) / QFX parser, field by field as in the design spec §6.3.
//!
//! OFX 1.x is not XML: leaf elements are not closed and aggregates may or may not be. A tag
//! scanner of its own reads both versions into the same [`Node`] tree the CAMT parser walks,
//! so the lookups (`at`, `text_at`) are the same. Header lines, the XML prolog and the
//! Intuit extensions (QFX) are of no interest: the text before `<OFX` is dropped and unknown
//! elements are never looked at. Leaves close implicitly, aggregates must be closed: a file that
//! ends inside one is refused as truncated. One `STMTRS` or `CCSTMTRS` is one [`Statement`].

use std::borrow::Cow;

use chrono::NaiveDate;

use crate::error::{AppError, Result};
use crate::services::csv_import::DeclaredBalance;
use crate::services::dedup::canonical_amount;

use super::decimal::Dec;
use super::model::{malformed, utc_day, AccountRef, Entry, Statement};
use super::reject_doctype;
use super::types::StatementWarning;
use super::xml_tree::{count_node, Node, MAX_DEPTH, MAX_NODES};

/// Row message key of a `STMTTRN` that corrects an earlier one (`CORRECTACTION`).
const CORRECTION_SKIPPED: &str = "statementImport.correctionSkipped";

/// Warning key of a file that carries investment statements next to bank or card ones.
const INVESTMENT_IGNORED: &str = "statementImport.investmentIgnored";

/// Longest entity body (`nbsp`, `#x10FFFF`) looked for between `&` and `;`.
const MAX_ENTITY_BYTES: usize = 12;

/// Longest element name in the detail of "the file ends inside ...".
const MAX_NAME_IN_DETAIL: usize = 40;

/// The OFX elements that hold a value and never contain other elements. SGML needs no closing
/// tag for them, so one without a value (`<BRANCHID>` and a line break) would otherwise stay
/// open and swallow the elements after it. Any other element is read as an aggregate.
const LEAVES: &[&str] = &[
    "BANKID",
    "BRANCHID",
    "ACCTID",
    "ACCTTYPE",
    "ACCTKEY",
    "TRNTYPE",
    "DTPOSTED",
    "DTUSER",
    "DTAVAIL",
    "TRNAMT",
    "FITID",
    "CORRECTFITID",
    "CORRECTACTION",
    "SRVRTID",
    "CHECKNUM",
    "REFNUM",
    "SIC",
    "PAYEEID",
    "NAME",
    "MEMO",
    "CURSYM",
    "CURRATE",
    "CURDEF",
    "BALAMT",
    "DTASOF",
    "DTSTART",
    "DTEND",
    "CODE",
    "SEVERITY",
    "DTSERVER",
    "LANGUAGE",
    "TRNUID",
    "EXTDNAME",
    "INV401KSOURCE",
    "MKTGINFO",
    "MESSAGE",
    "CLTCOOKIE",
    "DTPROFUP",
    "DTACCTUP",
    "ORG",
    "FID",
];

/// The statements of one OFX/QFX file and what was left out of them.
#[derive(Debug)]
pub struct OfxParse {
    pub statements: Vec<Statement>,
    pub warnings: Vec<StatementWarning>,
}

/// Parses an OFX or QFX document into its bank and credit card statements, in file order.
/// A file with only investment statements is `validation.statementInvestmentOnly`; one with
/// both imports the first kind and warns about the other.
pub fn parse(text: &str) -> Result<OfxParse> {
    reject_doctype(text)?;
    let root = parse_tree(text)?;
    let mut found = Vec::new();
    let mut investment = false;
    find_statements(&root, &mut found, &mut investment);
    if found.is_empty() {
        return Err(AppError::Validation(
            if investment {
                "validation.statementInvestmentOnly"
            } else {
                "validation.statementNoStatements"
            }
            .into(),
        ));
    }
    let statements = found
        .into_iter()
        .map(statement)
        .collect::<Result<Vec<_>>>()?;
    let warnings = if investment {
        vec![StatementWarning {
            key: INVESTMENT_IGNORED.to_string(),
            detail: None,
        }]
    } else {
        Vec::new()
    };
    Ok(OfxParse {
        statements,
        warnings,
    })
}

/// Every `STMTRS` and `CCSTMTRS` in document order, and whether an `INVSTMTRS` exists.
fn find_statements<'a>(node: &'a Node, found: &mut Vec<&'a Node>, investment: &mut bool) {
    for child in &node.children {
        match child.name.as_str() {
            "STMTRS" | "CCSTMTRS" => found.push(child),
            "INVSTMTRS" => *investment = true,
            _ => find_statements(child, found, investment),
        }
    }
}

fn statement(stmt: &Node) -> Result<Statement> {
    let currency = stmt.text_at(&["CURDEF"]).map(str::to_uppercase);
    let period =
        day_at(stmt, &["BANKTRANLIST", "DTSTART"]).zip(day_at(stmt, &["BANKTRANLIST", "DTEND"]));
    Ok(Statement {
        account: account_ref(stmt),
        closing: closing(stmt, currency.as_deref()),
        period,
        currency,
        entries: entries(stmt)?,
        ..Statement::default()
    })
}

/// `BANKACCTFROM` (`ACCTID` and the bank code `BANKID`) or `CCACCTFROM` (`ACCTID`).
fn account_ref(stmt: &Node) -> AccountRef {
    let from = stmt
        .child("BANKACCTFROM")
        .or_else(|| stmt.child("CCACCTFROM"));
    AccountRef {
        identifier: from
            .and_then(|from| from.text_at(&["ACCTID"]))
            .map(str::to_string),
        bank_code: from
            .and_then(|from| from.text_at(&["BANKID"]))
            .map(str::to_string),
        name: None,
    }
}

/// `LEDGERBAL`, in the statement's currency. OFX has no opening balance, so the balance check
/// of a file is `unavailable`. `None` without a currency, a readable amount or a day.
fn closing(stmt: &Node, currency: Option<&str>) -> Option<DeclaredBalance> {
    let balance = stmt.child("LEDGERBAL")?;
    Some(DeclaredBalance {
        amount: signed_amount(balance.text_at(&["BALAMT"])?)?,
        currency: currency?.to_string(),
        as_of: day(balance.text_at(&["DTASOF"])?)?,
    })
}

fn entries(stmt: &Node) -> Result<Vec<Entry>> {
    stmt.at(&["BANKTRANLIST"])
        .into_iter()
        .flat_map(|list| list.children("STMTTRN"))
        .enumerate()
        .map(|(index, trn)| {
            transaction(trn).ok_or_else(|| malformed(&format!("STMTTRN {}", index + 1)))
        })
        .collect()
}

/// One `STMTTRN`. `None` when the amount or the posting day cannot be read.
fn transaction(trn: &Node) -> Option<Entry> {
    let amount = signed_amount(trn.text_at(&["TRNAMT"])?)?;
    let (is_credit, amount) = match amount.strip_prefix('-') {
        Some(unsigned) => (false, unsigned.to_string()),
        None => (true, amount),
    };
    let name = trn
        .text_at(&["NAME"])
        .or_else(|| trn.text_at(&["PAYEE", "NAME"]));
    Some(Entry {
        booking_date: day(trn.text_at(&["DTPOSTED"])?)?,
        amount,
        is_credit,
        currency: trn.text_at(&["CURRENCY", "CURSYM"]).map(str::to_uppercase),
        bank_id: trn.text_at(&["FITID"]).map(str::to_string),
        description: trn.text_at(&["MEMO"]).or(name).map(str::to_string),
        counterparty_name: name.map(str::to_string),
        skip_reason: trn
            .child("CORRECTACTION")
            .is_some()
            .then_some(CORRECTION_SKIPPED),
        ..Entry::default()
    })
}

/// A signed amount as canonical decimal text. One `,` and no `.` is a decimal comma. `None`
/// unless the text is a plain decimal: `canonical_amount` answers "0" for text without
/// digits, which must not pass.
fn signed_amount(text: &str) -> Option<String> {
    let text = text.trim();
    let text = if text.matches(',').count() == 1 && !text.contains('.') {
        Cow::Owned(text.replace(',', "."))
    } else {
        Cow::Borrowed(text)
    };
    Dec::parse(&text)?;
    Some(canonical_amount(&text))
}

fn day_at(node: &Node, path: &[&str]) -> Option<i64> {
    day(node.text_at(path)?)
}

/// The calendar day of an OFX date-time (`YYYYMMDD[HHMMSS[.XXX][gmt offset]]`): its first
/// eight characters, never shifted by the time zone (ADR 0008).
fn day(text: &str) -> Option<i64> {
    let digits = text.trim().get(..8)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    NaiveDate::parse_from_str(digits, "%Y%m%d")
        .ok()
        .map(utc_day)
}

/// Reads a whole document into its `OFX` element. Text before `<OFX` (the header lines or the
/// XML prolog) is dropped. A document nested deeper than [`MAX_DEPTH`] or with more than
/// [`MAX_NODES`] elements, one that ends inside a tag and one that ends inside an aggregate
/// (a truncated file) is `validation.statementMalformed`.
fn parse_tree(text: &str) -> Result<Node> {
    parse_tree_with_limits(text, MAX_DEPTH, MAX_NODES)
}

fn parse_tree_with_limits(text: &str, max_depth: usize, max_nodes: usize) -> Result<Node> {
    let start = ofx_start(text).ok_or_else(|| malformed("OFX"))?;
    let mut rest = &text[start..];
    let mut tree = Tree::new(max_depth, max_nodes);
    while !rest.is_empty() {
        rest = if tag_starts_at(rest.as_bytes(), 0) {
            tree.tag(&rest[1..])?
        } else {
            let end = text_end(rest);
            tree.text(&decode_entities(&rest[..end]));
            &rest[end..]
        };
    }
    tree.finish()
}

/// A `<` starts a tag only before a letter, `/`, `!` or `?`. Any other (`A < B`, `1<2`) is a
/// character of the text.
fn tag_starts_at(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&b'<')
        && bytes
            .get(at + 1)
            .is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, b'/' | b'!' | b'?'))
}

/// Where the text at the start of `text` ends: at the next tag, else at the end.
fn text_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    text.match_indices('<')
        .map(|(at, _)| at)
        .find(|at| tag_starts_at(bytes, *at))
        .unwrap_or(text.len())
}

/// Byte offset of the first `<OFX` that is the element (followed by `>`, `/` or white space),
/// in any letter case.
fn ofx_start(text: &str) -> Option<usize> {
    const OPEN: &[u8] = b"<ofx";
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = bytes[from..]
        .windows(OPEN.len())
        .position(|window| window.eq_ignore_ascii_case(OPEN))
    {
        let at = from + found;
        match bytes.get(at + OPEN.len()) {
            Some(b'>' | b'/') => return Some(at),
            Some(next) if next.is_ascii_whitespace() => return Some(at),
            _ => from = at + 1,
        }
    }
    None
}

/// The elements being built. `stack[0]` is a root that only holds the `OFX` element; the
/// last one is the open element text belongs to.
struct Tree {
    stack: Vec<Node>,
    elements: usize,
    max_depth: usize,
    max_nodes: usize,
}

impl Tree {
    fn new(max_depth: usize, max_nodes: usize) -> Tree {
        Tree {
            stack: vec![Node::default()],
            elements: 0,
            max_depth,
            max_nodes,
        }
    }

    /// Reads the tag whose `<` was just consumed and returns the text after it.
    fn tag<'a>(&mut self, after_bracket: &'a str) -> Result<&'a str> {
        if let Some(comment) = after_bracket.strip_prefix("!--") {
            return comment
                .split_once("-->")
                .map(|(_, rest)| rest)
                .ok_or_else(|| malformed("tag"));
        }
        if let Some(cdata) = after_bracket.strip_prefix("![CDATA[") {
            let (content, rest) = cdata.split_once("]]>").ok_or_else(|| malformed("tag"))?;
            self.text(content);
            return Ok(rest);
        }
        let (inner, rest) = after_bracket
            .split_once('>')
            .ok_or_else(|| malformed("tag"))?;
        if inner.starts_with(['?', '!']) {
            return Ok(rest);
        }
        let (closing, inner) = match inner.strip_prefix('/') {
            Some(name) => (true, name),
            None => (false, inner),
        };
        let empty_element = !closing && inner.ends_with('/');
        let name = inner
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .map(str::to_ascii_uppercase);
        match name {
            Some(name) if closing => self.close(&name),
            Some(name) => {
                self.open(name.clone())?;
                if empty_element {
                    self.close(&name);
                }
            }
            None => {}
        }
        Ok(rest)
    }

    /// Opens an element. No OFX aggregate contains itself, so an element whose name is already
    /// open closes that one (and what is unclosed above it) first: `<STMTTRN>` after an unclosed
    /// `<STMTTRN>` is its next sibling, not its child.
    fn open(&mut self, name: String) -> Result<()> {
        self.close(&name);
        // The root is no element.
        if self.stack.len() > self.max_depth {
            return Err(malformed(&name));
        }
        count_node(&mut self.elements, self.max_nodes)?;
        self.stack.push(Node {
            name,
            ..Node::default()
        });
        Ok(())
    }

    /// A value runs until the next tag: the element on top is a leaf from its first non-blank
    /// text on. Text of an element that already has children is not a value and is dropped.
    fn text(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.stack.len() < 2 {
            return;
        }
        if let Some(top) = self.stack.last_mut().filter(|top| top.children.is_empty()) {
            top.text.push_str(text);
        }
    }

    /// A closing tag ends the leaf it follows (implicitly or as its own `</NAME>`) and every
    /// unclosed element above the open element of that name. Without such an element it is
    /// ignored.
    fn close(&mut self, name: &str) {
        self.close_leaf();
        if let Some(depth) = self.stack.iter().rposition(|open| open.name == name) {
            if depth > 0 {
                while self.stack.len() > depth {
                    self.pop();
                }
            }
        }
    }

    /// Ends the element on top when it is a leaf, which needs no closing tag: one that holds a
    /// value, or an empty one of the OFX elements that never contain others ([`LEAVES`]).
    fn close_leaf(&mut self) {
        let leaf = |top: &Node| {
            !top.text.is_empty() || (top.children.is_empty() && LEAVES.contains(&top.name.as_str()))
        };
        if self.stack.len() > 1 && self.stack.last().is_some_and(leaf) {
            self.pop();
        }
    }

    fn pop(&mut self) {
        if self.stack.len() < 2 {
            return;
        }
        if let Some(node) = self.stack.pop() {
            if let Some(parent) = self.stack.last_mut() {
                parent.children.push(node);
            }
        }
    }

    /// Returns the `OFX` element. Leaves need no closing tag, aggregates do: a text that ends
    /// inside one is a truncated file, which must not import what happens to precede the cut.
    fn finish(mut self) -> Result<Node> {
        self.close_leaf();
        if let Some(open) = self.stack.last().filter(|_| self.stack.len() > 1) {
            let name: String = open.name.chars().take(MAX_NAME_IN_DETAIL).collect();
            return Err(malformed(&format!("OFX ends inside <{name}>")));
        }
        let root = self.stack.pop().unwrap_or_default();
        root.children
            .into_iter()
            .find(|node| node.name == "OFX")
            .ok_or_else(|| malformed("OFX"))
    }
}

/// Replaces `&amp; &lt; &gt; &quot; &apos; &nbsp;` and `&#NNN;` / `&#xHH;`. OFX is not strict
/// XML: an entity this does not know, a `&` without `;` and a reference to no character
/// stay as written.
fn decode_entities(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        decoded.push_str(&rest[..at]);
        rest = &rest[at..];
        match entity(rest) {
            Some((character, length)) => {
                decoded.push(character);
                rest = &rest[length..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    Cow::Owned(decoded)
}

/// The character the entity at the start of `text` (which starts with `&`) stands for and
/// the entity's length in bytes.
fn entity(text: &str) -> Option<(char, usize)> {
    let end = text
        .as_bytes()
        .iter()
        .take(MAX_ENTITY_BYTES)
        .position(|byte| *byte == b';')?;
    let body = &text[1..end];
    let character = match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => {
            let reference = body.strip_prefix('#')?;
            let (digits, radix) = match reference.strip_prefix(['x', 'X']) {
                Some(hex) => (hex, 16),
                None => (reference, 10),
            };
            if digits.is_empty() || !digits.chars().all(|digit| digit.is_digit(radix)) {
                return None;
            }
            char::from_u32(u32::from_str_radix(digits, radix).ok()?).filter(|c| *c != '\0')?
        }
    };
    Some((character, end + 1))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::super::model::{utc_day, Entry};
    use super::*;
    use crate::error::AppError;

    fn day(y: i32, m: u32, d: u32) -> i64 {
        utc_day(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }
    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/statements/ofx/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }
    fn error_key(error: &AppError) -> String {
        error.keyed().expect("keyed").0.to_string()
    }
    fn detail(error: &AppError) -> String {
        let (key, params) = error.keyed().expect("keyed");
        assert_eq!(key, "validation.statementMalformed");
        params["detail"].as_str().expect("detail").to_string()
    }

    /// A one-statement bank file whose first `STMTTRN` has the given body.
    fn bank_file(transactions: &str) -> String {
        format!(
            "<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>USD<BANKACCTFROM><BANKID>1<ACCTID>2</BANKACCTFROM>\
             <BANKTRANLIST>{transactions}</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>"
        )
    }
    fn transaction(body: &str) -> String {
        format!("<STMTTRN>{body}</STMTTRN>")
    }
    fn entries(body: &str) -> Vec<Entry> {
        parse(&bank_file(&transaction(body)))
            .unwrap()
            .statements
            .remove(0)
            .entries
    }
    fn entry(body: &str) -> Entry {
        entries(body).remove(0)
    }
    /// A bank statement with the given content and nothing else around it.
    fn bare_statement(inner: &str) -> Statement {
        parse(&format!(
            "<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS>{inner}</STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>"
        ))
        .unwrap()
        .statements
        .remove(0)
    }
    fn amount_of(text: &str) -> (String, bool) {
        let e = entry(&format!("<DTPOSTED>20260105<TRNAMT>{text}"));
        (e.amount, e.is_credit)
    }

    #[test]
    fn sgml_checking_statement() {
        let parsed = parse(&fixture("checking-sgml.ofx")).unwrap();
        let s = &parsed.statements[0];
        assert_eq!(
            (
                s.account.identifier.as_deref(),
                s.account.bank_code.as_deref()
            ),
            (Some("123456789"), Some("021000021"))
        );
        assert_eq!(s.currency.as_deref(), Some("USD"));
        assert_eq!(s.period, Some((day(2026, 9, 1), day(2026, 9, 30))));
        assert_eq!(s.opening, None);
        assert_eq!(
            s.closing.as_ref().map(|b| (b.amount.as_str(), b.as_of)),
            Some(("1250.5", day(2026, 9, 30)))
        );
        let e = &s.entries;
        assert_eq!(
            (
                e[0].amount.as_str(),
                e[0].is_credit,
                e[0].bank_id.as_deref()
            ),
            ("500.5", true, Some("F1"))
        );
        assert_eq!(e[0].booking_date, day(2026, 9, 5));
        assert_eq!(e[0].counterparty_name.as_deref(), Some("ACME & SONS <US>"));
        assert_eq!(e[0].description.as_deref(), Some("Invoice 17"));
        assert_eq!(
            (e[1].amount.as_str(), e[1].is_credit),
            ("250", false),
            "decimal comma, trailing spaces"
        );
        assert_eq!(e[1].counterparty_name.as_deref(), Some("GROCERY"));
        assert_eq!(e[1].description.as_deref(), Some("GROCERY"));
        assert_eq!(e[2].skip_reason, Some("statementImport.correctionSkipped"));
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn xml_credit_card_with_char_refs() {
        let s = &parse(&fixture("card-xml.qfx")).unwrap().statements[0];
        assert_eq!(
            s.account.identifier.as_deref(),
            Some("DE89370400440532013000")
        );
        assert_eq!(s.account.bank_code, None);
        assert_eq!(s.currency.as_deref(), Some("EUR"));
        assert_eq!(
            s.closing
                .as_ref()
                .map(|b| (b.amount.as_str(), b.currency.as_str())),
            Some(("-12.3", "EUR"))
        );
        assert_eq!(
            s.entries[0].counterparty_name.as_deref(),
            Some("Café Müller")
        );
        assert!(!s.entries[0].is_credit);
    }

    #[test]
    fn investment_only_is_refused() {
        assert!(parse(&fixture("investment-only.ofx"))
            .unwrap_err()
            .to_string()
            .contains("validation.statementInvestmentOnly"));
    }

    #[test]
    fn mixed_file_warns_about_investments() {
        let text = fixture("checking-sgml.ofx").replace(
            "</OFX>",
            "<INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS></INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1></OFX>",
        );
        let parsed = parse(&text).unwrap();
        assert_eq!(parsed.warnings[0].key, "statementImport.investmentIgnored");
        assert_eq!(parsed.warnings[0].detail, None);
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.statements.len(), 1);
    }

    #[test]
    fn entities_decode_and_unknown_ones_stay_literal() {
        assert_eq!(
            decode_entities("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"),
            "a & b <c> \"d\" 'e'"
        );
        assert_eq!(decode_entities("x&nbsp;y"), "x\u{a0}y");
        assert_eq!(decode_entities("&#233;&#xE9;&#XE9;&#65;"), "éééA");
        assert_eq!(decode_entities("&amp;amp;"), "&amp;", "no second decoding");
        for literal in [
            "AT&T",
            "&foo;",
            "a & b",
            "&#;",
            "&#x;",
            "&#xZZ;",
            "&#0;",
            "&#xD800;",
            "&#1114112;",
            "&#+65;",
            "&amp",
            "trailing &",
            "&averyveryverylongname;",
        ] {
            assert_eq!(decode_entities(literal), literal);
        }
        // the same through a value
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>AT&amp;T &foo; &#233;");
        assert_eq!(e.counterparty_name.as_deref(), Some("AT&T &foo; é"));
    }

    #[test]
    fn tag_names_are_upper_cased_and_leaves_close_implicitly() {
        let e = entry("<dtposted>20260105<trnamt>-5<fitid>x");
        assert_eq!(
            (e.booking_date, e.amount.as_str(), e.bank_id.as_deref()),
            (day(2026, 1, 5), "5", Some("x"))
        );
    }

    #[test]
    fn the_tree_closes_leaves_implicitly_and_skips_stray_closing_tags() {
        let root = parse_tree("<OFX><A><B>1<C>2</B><D>3</D></A><E> 4 </E></OFX>").unwrap();
        assert_eq!(root.name, "OFX");
        // `<C>` closed the leaf B, so the `</B>` after C's value is skipped: C stays B's sibling
        assert_eq!(root.text_at(&["A", "B"]), Some("1"));
        assert_eq!(root.text_at(&["A", "C"]), Some("2"));
        assert_eq!(root.text_at(&["A", "D"]), Some("3"));
        assert_eq!(root.text_at(&["E"]), Some("4"));
        assert!(root.at(&["A", "B", "C"]).is_none());

        // a closing tag with no open element is ignored and does not end the aggregate
        let root = parse_tree("<OFX><A><B>1</B></Z><C>2</C></A></OFX>").unwrap();
        assert_eq!(root.text_at(&["A", "C"]), Some("2"));

        // a closing tag pops every unclosed element above its match
        let root = parse_tree("<OFX><A><B><C>1</A><D>2</D></OFX>").unwrap();
        assert_eq!(root.text_at(&["A", "B", "C"]), Some("1"));
        assert_eq!(root.text_at(&["D"]), Some("2"));

        // an element without text and without a closing tag stays open until its parent closes
        let root = parse_tree("<OFX><A><B></A><C>1</C></OFX>").unwrap();
        assert!(root.at(&["A", "B"]).is_some());
        assert_eq!(root.text_at(&["C"]), Some("1"));

        // a leaf at the end of the text needs no closing tag
        let root = parse_tree("<OFX><A>1</A></OFX><B>2").unwrap();
        assert_eq!(root.text_at(&["A"]), Some("1"));
    }

    #[test]
    fn a_file_that_ends_inside_an_aggregate_is_refused() {
        let ends_inside = |text: &str| detail(&parse_tree(text).unwrap_err());
        assert_eq!(ends_inside("<OFX><A><B>1"), "OFX ends inside <A>");
        assert_eq!(ends_inside("<OFX><A>"), "OFX ends inside <A>");
        assert_eq!(ends_inside("<OFX><A><B>1</A>"), "OFX ends inside <OFX>");
        // a leaf holding a value is not what the text ends inside
        assert_eq!(ends_inside("<OFX><A><B>1</B><C>2"), "OFX ends inside <A>");
        // the name comes from the file and is cut
        let long = ends_inside(&format!("<OFX><{}>", "n".repeat(10_000)));
        assert!(long.starts_with("OFX ends inside <NNN"), "{long}");
        assert!(long.chars().count() <= 64, "{long}");

        // the SGML statement cut in the middle of a transaction, and without its last tag
        let text = fixture("checking-sgml.ofx");
        assert!(parse(&text).is_ok());
        let cut = text.find("<FITID>F2").unwrap();
        assert_eq!(
            detail(&parse(&text[..cut]).unwrap_err()),
            "OFX ends inside <STMTTRN>"
        );
        let after_list = text.find("</BANKTRANLIST>").unwrap();
        assert_eq!(
            detail(&parse(&text[..after_list]).unwrap_err()),
            "OFX ends inside <BANKTRANLIST>"
        );
        let without_end = text.replace("</OFX>", "");
        assert_eq!(
            detail(&parse(&without_end).unwrap_err()),
            "OFX ends inside <OFX>"
        );
    }

    #[test]
    fn empty_and_blank_leaves_do_not_swallow_their_siblings() {
        // an empty `<BRANCHID>` between the bank code and the account number
        let account = bare_statement(
            "<BANKACCTFROM><BANKID>0000<BRANCHID><ACCTID>123456789<ACCTTYPE>CHECKING</BANKACCTFROM>",
        )
        .account;
        assert_eq!(
            (account.identifier.as_deref(), account.bank_code.as_deref()),
            (Some("123456789"), Some("0000"))
        );
        // a blank one, with a line break before the next tag
        let account = bare_statement(
            "<BANKACCTFROM><BANKID>0000<BRANCHID>   \r\n<ACCTID>123456789</BANKACCTFROM>",
        )
        .account;
        assert_eq!(account.identifier.as_deref(), Some("123456789"));
        // the same through the tree
        let root = parse_tree("<OFX><A><BRANCHID><ACCTID>1</A></OFX>").unwrap();
        assert!(root.at(&["A", "BRANCHID"]).is_some());
        assert_eq!(root.text_at(&["A", "ACCTID"]), Some("1"));
        assert!(root.at(&["A", "BRANCHID", "ACCTID"]).is_none());

        // an empty check number before the payee, a memo after an empty or blank name
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<CHECKNUM><NAME>Shop<MEMO>Note");
        assert_eq!(
            (e.counterparty_name.as_deref(), e.description.as_deref()),
            (Some("Shop"), Some("Note"))
        );
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME><MEMO>Note");
        assert_eq!(
            (e.counterparty_name.as_deref(), e.description.as_deref()),
            (None, Some("Note"))
        );
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>   \r\n<MEMO>Note<FITID>X");
        assert_eq!(
            (
                e.counterparty_name.as_deref(),
                e.description.as_deref(),
                e.bank_id.as_deref()
            ),
            (None, Some("Note"), Some("X"))
        );
        // an empty value in the middle of the fields does not hide the amount that follows
        let e = entry("<DTPOSTED>20260105<FITID><TRNAMT>-7<MEMO>");
        assert_eq!(
            (e.amount.as_str(), e.is_credit, e.bank_id),
            ("7", false, None)
        );
        // explicit closing tags of empty leaves are as good
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME></NAME><MEMO>Note</MEMO>");
        assert_eq!(e.description.as_deref(), Some("Note"));
    }

    #[test]
    fn an_unclosed_aggregate_is_closed_by_the_next_one_of_its_name() {
        // no OFX aggregate contains itself: three transactions without `</STMTTRN>` are three
        let three = parse(&bank_file(
            "<STMTTRN><DTPOSTED>20260105<TRNAMT>1<FITID>A\
             <STMTTRN><DTPOSTED>20260106<TRNAMT>2<FITID>B\
             <STMTTRN><DTPOSTED>20260107<TRNAMT>3<FITID>C",
        ))
        .unwrap()
        .statements
        .remove(0)
        .entries;
        assert_eq!(
            three
                .iter()
                .map(|e| (e.amount.as_str(), e.bank_id.as_deref()))
                .collect::<Vec<_>>(),
            vec![("1", Some("A")), ("2", Some("B")), ("3", Some("C"))]
        );

        // one unclosed one followed by a closed one
        let two = parse(&bank_file(
            "<STMTTRN><DTPOSTED>20260105<TRNAMT>1<FITID>A\
             <STMTTRN><DTPOSTED>20260106<TRNAMT>2<FITID>B</STMTTRN>",
        ))
        .unwrap()
        .statements
        .remove(0)
        .entries;
        assert_eq!(two.len(), 2);
        assert_eq!(two[1].bank_id.as_deref(), Some("B"));

        // the same through the tree, with an open leaf between them
        let root = parse_tree("<OFX><L><T><M>a<T><M>b<T><M>c</L></OFX>").unwrap();
        let list = root.child("L").unwrap();
        assert_eq!(list.children("T").count(), 3);
        assert!(root.at(&["L", "T", "T"]).is_none());
    }

    #[test]
    fn an_empty_extension_leaf_does_not_swallow_the_correction() {
        // reviewer probe: an empty `<EXTDNAME>` before the `<FITID>` and the `<CORRECTACTION>`
        let e = entry("<DTPOSTED>20260106<TRNAMT>-1<EXTDNAME><FITID>B<CORRECTACTION>DELETE");
        assert_eq!(e.bank_id.as_deref(), Some("B"));
        assert_eq!(e.skip_reason, Some("statementImport.correctionSkipped"));
        // every added leaf closes empty
        for leaf in [
            "EXTDNAME",
            "INV401KSOURCE",
            "MKTGINFO",
            "MESSAGE",
            "CLTCOOKIE",
            "DTPROFUP",
            "DTACCTUP",
            "ORG",
            "FID",
        ] {
            let root = parse_tree(&format!("<OFX><A><{leaf}><ACCTID>1</A></OFX>")).unwrap();
            assert_eq!(root.text_at(&["A", "ACCTID"]), Some("1"), "{leaf}");
        }
    }

    #[test]
    fn a_less_than_sign_that_starts_no_tag_is_text() {
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>A < B<MEMO>x");
        assert_eq!(
            (e.counterparty_name.as_deref(), e.description.as_deref()),
            (Some("A < B"), Some("x"))
        );
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>1<2 and 3<=4 <é<MEMO>x");
        assert_eq!(e.counterparty_name.as_deref(), Some("1<2 and 3<=4 <é"));
        let e = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>x<<MEMO>y");
        assert_eq!(
            (e.counterparty_name.as_deref(), e.description.as_deref()),
            (Some("x<"), Some("y"))
        );
        // at the very end of the text
        let root = parse_tree("<OFX><A>1 <</A></OFX>").unwrap();
        assert_eq!(root.text_at(&["A"]), Some("1 <"));
    }

    #[test]
    fn everything_before_ofx_is_dropped() {
        let root = parse_tree(
            "OFXHEADER:100\nDATA:OFXSGML\n\u{feff}<?xml version=\"1.0\"?>\n<?OFX OFXHEADER=\"200\"?>\n junk < <OFXHEADER> <OfX>\n<A>1</A></ofx>",
        )
        .unwrap();
        assert_eq!(root.name, "OFX");
        assert_eq!(root.text_at(&["A"]), Some("1"));
        assert_eq!(
            detail(&parse_tree("OFXHEADER:100\n\n<STMTTRN>").unwrap_err()),
            "OFX"
        );
        assert_eq!(
            detail(&parse_tree("<OFXISH><A>1</A></OFXISH>").unwrap_err()),
            "OFX"
        );
    }

    #[test]
    fn comments_processing_instructions_cdata_and_empty_elements() {
        let root = parse_tree(
            "<OFX><!-- note --><?pi x?><A><![CDATA[<raw> & x]]></A><B/><C></C><!DOCUMENT x><D x=\"1\">2</D></OFX>",
        )
        .unwrap();
        assert_eq!(root.text_at(&["A"]), Some("<raw> & x"));
        assert!(root.at(&["B"]).is_some() && root.text_at(&["B"]).is_none());
        assert!(root.at(&["C"]).is_some());
        assert_eq!(root.text_at(&["D"]), Some("2"));
        // unterminated constructs are damage, not data
        for broken in ["<OFX><A", "<OFX><!-- x", "<OFX><![CDATA[x", "<OFX><A>1</"] {
            assert_eq!(detail(&parse_tree(broken).unwrap_err()), "tag", "{broken}");
        }
    }

    #[test]
    fn absurd_nesting_and_size_are_refused() {
        // the deep text must be over the cap, whatever the XML tree's caps become
        const LEVELS: usize = 10_000;
        const _: () = assert!(MAX_DEPTH < LEVELS);
        // distinct names: an element opened inside one of its own name closes it instead
        let deep: String = std::iter::once("<OFX>".to_string())
            .chain((0..LEVELS).map(|level| format!("<a{level}>")))
            .collect();
        let name = detail(&parse_tree(&deep).unwrap_err());
        assert!(name.starts_with('A') && name.len() > 1, "{name}");
        let tree = "<OFX><a><b><c>y</c></b></a><d/></OFX>"; // OFX a b c d: 5 elements, 4 deep
        assert!(parse_tree_with_limits(tree, 4, 5).is_ok());
        assert_eq!(
            detail(&parse_tree_with_limits(tree, 3, 5).unwrap_err()),
            "C"
        );
        let error = parse_tree_with_limits(tree, 4, 4).unwrap_err();
        assert!(
            detail(&error).contains("too many elements"),
            "{}",
            detail(&error)
        );
        let flat = format!("<OFX>{}</OFX>", "<e/>".repeat(100));
        assert!(parse_tree_with_limits(&flat, 128, 101).is_ok());
        assert!(parse_tree_with_limits(&flat, 128, 100).is_err());
    }

    #[test]
    fn amounts_with_a_decimal_comma_or_a_sign() {
        assert_eq!(amount_of("1,5"), ("1.5".into(), true));
        assert_eq!(amount_of("-1234,50"), ("1234.5".into(), false));
        assert_eq!(amount_of("+12.50"), ("12.5".into(), true));
        assert_eq!(amount_of("-0.001"), ("0.001".into(), false));
        assert_eq!(amount_of(" 7 "), ("7".into(), true));
        for bad in [
            "1,000.50", "1,000,50", "1 000.50", "abc", "", "-", "1e3", "5 USD",
        ] {
            let error = parse(&bank_file(&transaction(&format!(
                "<DTPOSTED>20260105<TRNAMT>{bad}"
            ))))
            .unwrap_err();
            assert_eq!(detail(&error), "STMTTRN 1", "{bad:?}");
        }
    }

    #[test]
    fn a_transaction_that_cannot_be_read_names_its_ordinal() {
        let good = transaction("<DTPOSTED>20260105<TRNAMT>1");
        for bad in [
            "<DTPOSTED>20260105",
            "<TRNAMT>1",
            "<DTPOSTED>2026-01-05<TRNAMT>1",
            "<DTPOSTED>20261301<TRNAMT>1",
            "<DTPOSTED>2026010<TRNAMT>1",
            "<DTPOSTED>2026+105<TRNAMT>1",
        ] {
            let error = parse(&bank_file(&format!("{good}{}", transaction(bad)))).unwrap_err();
            assert_eq!(detail(&error), "STMTTRN 2", "{bad}");
        }
    }

    #[test]
    fn dates_are_the_first_eight_characters() {
        let e = entry("<DTPOSTED>20261231235959.000[+9:JST]<TRNAMT>1");
        assert_eq!(e.booking_date, day(2026, 12, 31));
        assert_eq!(e.value_date, None);
    }

    #[test]
    fn name_memo_payee_and_currency() {
        let both = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>Shop<MEMO>Note");
        assert_eq!(both.counterparty_name.as_deref(), Some("Shop"));
        assert_eq!(both.description.as_deref(), Some("Note"));

        let payee = entry("<DTPOSTED>20260105<TRNAMT>1<PAYEE><NAME>Corner Shop<CITY>Town</PAYEE>");
        assert_eq!(payee.counterparty_name.as_deref(), Some("Corner Shop"));
        assert_eq!(payee.description.as_deref(), Some("Corner Shop"));

        let named = entry("<DTPOSTED>20260105<TRNAMT>1<NAME>Own<PAYEE><NAME>Other</PAYEE>");
        assert_eq!(named.counterparty_name.as_deref(), Some("Own"));

        let memo_only = entry("<DTPOSTED>20260105<TRNAMT>1<MEMO>Only a memo");
        assert_eq!(memo_only.counterparty_name, None);
        assert_eq!(memo_only.description.as_deref(), Some("Only a memo"));

        let none = entry("<DTPOSTED>20260105<TRNAMT>1<NAME></NAME><MEMO>  </MEMO>");
        assert_eq!((none.counterparty_name, none.description), (None, None));

        let foreign =
            entry("<DTPOSTED>20260105<TRNAMT>1<CURRENCY><CURRATE>1.2<CURSYM>eur</CURRENCY>");
        assert_eq!(foreign.currency.as_deref(), Some("EUR"));
        let plain = entry("<DTPOSTED>20260105<TRNAMT>1<ORIGCURRENCY><CURSYM>GBP</ORIGCURRENCY>");
        assert_eq!(plain.currency, None);
        assert!(!plain.pending);
    }

    #[test]
    fn a_correction_is_listed_but_not_applied() {
        let all = entries(
            "<DTPOSTED>20260105<TRNAMT>1<FITID>A</STMTTRN>\
             <STMTTRN><DTPOSTED>20260106<TRNAMT>-1<FITID>B<CORRECTFITID>A<CORRECTACTION>DELETE</STMTTRN>\
             <STMTTRN><DTPOSTED>20260107<TRNAMT>2<FITID>C",
        );
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter().map(|e| e.skip_reason).collect::<Vec<_>>(),
            vec![None, Some("statementImport.correctionSkipped"), None]
        );
    }

    #[test]
    fn statement_level_fields_are_optional() {
        let bare = bare_statement;
        let empty = bare("");
        assert_eq!(empty.account.identifier, None);
        assert_eq!(
            (empty.currency, empty.period, empty.closing),
            (None, None, None)
        );
        assert!(empty.entries.is_empty());

        // a balance needs the statement currency, an amount and a day
        let balance = "<LEDGERBAL><BALAMT>10,5<DTASOF>20260930</LEDGERBAL>";
        assert_eq!(bare(balance).closing, None, "no CURDEF");
        let with_currency = bare(&format!("<CURDEF>eur{balance}"));
        assert_eq!(with_currency.currency.as_deref(), Some("EUR"));
        let closing = with_currency.closing.unwrap();
        assert_eq!(
            (
                closing.amount.as_str(),
                closing.currency.as_str(),
                closing.as_of
            ),
            ("10.5", "EUR", day(2026, 9, 30))
        );
        for broken in [
            "<LEDGERBAL><BALAMT>10<DTASOF>soon</LEDGERBAL>",
            "<LEDGERBAL><BALAMT>ten<DTASOF>20260930</LEDGERBAL>",
            "<LEDGERBAL><DTASOF>20260930</LEDGERBAL>",
            "<LEDGERBAL><BALAMT>10</LEDGERBAL>",
        ] {
            assert_eq!(
                bare(&format!("<CURDEF>EUR{broken}")).closing,
                None,
                "{broken}"
            );
        }
        // the period needs both ends
        let list = |inner: &str| bare(&format!("<BANKTRANLIST>{inner}</BANKTRANLIST>")).period;
        assert_eq!(
            list("<DTSTART>20260901<DTEND>20260930"),
            Some((day(2026, 9, 1), day(2026, 9, 30)))
        );
        assert_eq!(list("<DTSTART>20260901"), None);
        assert_eq!(list("<DTSTART>20260901<DTEND>x"), None);
    }

    #[test]
    fn bank_and_card_statements_come_in_file_order() {
        let text = "<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>USD<BANKACCTFROM><BANKID>11<ACCTID>A</BANKACCTFROM></STMTRS></STMTTRNRS></BANKMSGSRSV1>\
                    <CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><CURDEF>EUR<CCACCTFROM><ACCTID>B</CCACCTFROM></CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1>\
                    <BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKACCTFROM><ACCTID>C</BANKACCTFROM></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>";
        let parsed = parse(text).unwrap();
        let ids: Vec<_> = parsed
            .statements
            .iter()
            .map(|s| {
                (
                    s.account.identifier.as_deref(),
                    s.account.bank_code.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                (Some("A"), Some("11")),
                (Some("B"), None),
                (Some("C"), None)
            ]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn files_without_a_statement() {
        let signon = "<OFX><SIGNONMSGSRSV1><SONRS><STATUS><CODE>15500<SEVERITY>ERROR</STATUS></SONRS></SIGNONMSGSRSV1></OFX>";
        assert_eq!(
            error_key(&parse(signon).unwrap_err()),
            "validation.statementNoStatements"
        );
        assert_eq!(
            detail(&parse("OFXHEADER:100\n\nnothing").unwrap_err()),
            "OFX"
        );
        assert_eq!(
            error_key(
                &parse("<?xml version=\"1.0\"?><!DOCTYPE OFX [<!ENTITY a \"b\">]><OFX></OFX>")
                    .unwrap_err()
            ),
            "validation.statementDoctype"
        );
        // an investment statement inside a file that also has no bank or card statement
        let investment = "<OFX><INVSTMTMSGSRSV1><INVSTMTTRNRS><INVSTMTRS></INVSTMTRS></INVSTMTTRNRS></INVSTMTMSGSRSV1></OFX>";
        assert_eq!(
            error_key(&parse(investment).unwrap_err()),
            "validation.statementInvestmentOnly"
        );
    }
}
