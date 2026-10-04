//! Shared test data for the stock import: the header rows of the supported
//! brokers (their public export shapes) and access to the fixture files. Each
//! test module uses a subset.

#![allow(dead_code)]

/// `src-tauri/tests/fixtures/csv/`.
pub const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/csv");

pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = format!("{FIXTURE_DIR}/{name}");
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read fixture {path}: {e}"))
}

/// `&["a", "b"]` → owned header cells.
pub fn headers(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|c| c.to_string()).collect()
}

/// Data rows from string slices (every row is its own array).
pub fn rows(data: &[&[&str]]) -> Vec<Vec<String>> {
    data.iter().map(|r| headers(r)).collect()
}

pub const XTB_HEADERS: [&str; 6] = ["ID", "Type", "Time", "Symbol", "Comment", "Amount"];

pub const TRADING212_HEADERS: [&str; 19] = [
    "Action",
    "Time",
    "ISIN",
    "Ticker",
    "Name",
    "No. of shares",
    "Price / share",
    "Currency (Price / share)",
    "Exchange rate",
    "Result",
    "Currency (Result)",
    "Total",
    "Currency (Total)",
    "Withholding tax",
    "Currency (Withholding tax)",
    "Notes",
    "ID",
    "Currency conversion fee",
    "Currency (Currency conversion fee)",
];

/// The English Transactions export: the unnamed columns hold the currency of
/// the amount before them.
pub const DEGIRO_HEADERS: [&str; 19] = [
    "Date",
    "Time",
    "Product",
    "ISIN",
    "Reference exchange",
    "Venue",
    "Quantity",
    "Price",
    "",
    "Local value",
    "",
    "Value",
    "",
    "Exchange rate",
    "Transaction and/or third party fees",
    "",
    "Total",
    "",
    "Order ID",
];

/// Newer exports insert the AutoFX fee after the exchange rate.
pub const DEGIRO_AUTOFX_HEADERS: [&str; 20] = [
    "Date",
    "Time",
    "Product",
    "ISIN",
    "Reference exchange",
    "Venue",
    "Quantity",
    "Price",
    "",
    "Local value",
    "",
    "Value",
    "",
    "Exchange rate",
    "AutoFX Fee",
    "Transaction and/or third party fees",
    "",
    "Total",
    "",
    "Order ID",
];

pub const IBKR_HEADERS: [&str; 9] = [
    "Symbol",
    "ISIN",
    "TradeDate",
    "Buy/Sell",
    "Quantity",
    "TradePrice",
    "CurrencyPrimary",
    "AssetClass",
    "TradeID",
];

pub const MOONY_HEADERS_EN: [&str; 7] = [
    "Date", "Type", "Ticker", "Name", "Quantity", "Price", "Currency",
];

pub const MOONY_HEADERS_CS: [&str; 7] =
    ["Datum", "Typ", "Symbol", "Název", "Počet", "Cena", "Měna"];

/// Degiro's other UI languages translate the names and keep the layout.
pub const DEGIRO_HEADERS_NL: [&str; 19] = [
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

pub const DEGIRO_HEADERS_CS: [&str; 19] = [
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
