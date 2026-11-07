//! Bank operations: accounts, payments, balance records, statement imports and their batches,
//! categories, learned payees and learned CSV formats, and the synced profile columns.

use std::path::Path;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{by_content, currency, day, money, no_target, one_of, pick, pick2, run, OpFn, Res};
use crate::models::bank_accounts::{InsertBalanceRecord, InsertBankAccount};
use crate::models::bank_transactions::{
    InsertBankTransaction, InsertTransactionCategory, UpdateTransactionCategory,
};
use crate::models::categorization::InsertScopedLearning;
use crate::models::user::UpdateUserProfile;
use crate::services::categorization::engine::CategorizationEngine;
use crate::services::categorization::learn;
use crate::services::csv_import::types::{InsertLearnedCsvFormat, LearnedCsvMapping};
use crate::services::csv_import::{self, learned, CsvImportConfig};
use crate::services::{auth, bank_accounts, categories};

pub(super) const OPS: &[(&str, OpFn)] = &[
    ("bank_accounts::create_account", create_account),
    ("bank_accounts::update_account", update_account),
    ("bank_accounts::delete_account", delete_account),
    ("bank_accounts::create_transaction", create_transaction),
    ("bank_accounts::set_transaction_category", set_category),
    ("bank_accounts::record_balance", record_balance),
    (
        "bank_accounts::delete_balance_record",
        delete_balance_record,
    ),
    ("csv_import::import_transactions", import_statement),
    ("csv_import::delete_import_batch", delete_import_batch),
    ("csv_import::learned::save_format", learn_csv_format),
    ("categories::create_category", create_category),
    ("categories::update_category", update_category),
    ("categories::delete_category", delete_category),
    ("categorization::learn::learn_scoped", learn_scoped),
    ("auth::apply_profile_update", update_profile),
];

const ACCOUNT_NAMES: [&str; 4] = ["Main", "Savings", "Travel", "Joint"];
const CATEGORY_NAMES: [&str; 6] = ["Food", "Travel", "Rent", "Fun", "Gifts", "Groceries"];
const PAYEES: [&str; 4] = ["Albert", "Lidl", "Shell", "Landlord"];

fn accounts(select: &str) -> String {
    by_content(
        "bank_accounts",
        select,
        "",
        "t.name, t.currency, t.account_type, t.interest_rate",
    )
}

fn categories() -> String {
    by_content(
        "transaction_categories",
        "t.id",
        "",
        "t.name, t.icon, t.color",
    )
}

fn account_data(rng: &mut StdRng, currency: &str) -> InsertBankAccount {
    InsertBankAccount {
        name: one_of(rng, &ACCOUNT_NAMES).to_string(),
        account_type: Some(one_of(rng, &["checking", "savings"]).to_string()),
        iban: None,
        bban: None,
        currency: Some(currency.to_string()),
        balance: None,
        balance_as_of: None,
        institution_id: None,
        interest_rate: Some(format!("{}.5", rng.random_range(0..5))),
        has_zone_designation: None,
        termination_date: None,
        interest_rate_valid_until: None,
        exclude_from_balance: Some(rng.random_bool(0.2)),
    }
}

fn create_account(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let account_currency = currency(rng);
    let data = account_data(rng, account_currency);
    run(format!("{:?} {:?}", data.name, data.currency), || {
        data.validate()?;
        bank_accounts::create_account(conn, &data).map(drop)
    })
}

fn update_account(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &accounts("t.id"))? else {
        return no_target();
    };
    let account_currency = currency(rng);
    let data = account_data(rng, account_currency);
    run(
        format!("{id} -> {:?} {:?}", data.name, data.currency),
        || {
            data.validate()?;
            bank_accounts::update_account(conn, &id, &data).map(drop)
        },
    )
}

fn delete_account(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &accounts("t.id"))? else {
        return no_target();
    };
    run(id.clone(), || bank_accounts::delete_account(conn, &id))
}

fn create_transaction(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some((account, account_currency)) = pick2(conn, rng, &accounts("t.id, t.currency"))? else {
        return no_target();
    };
    let category = if rng.random_bool(0.5) {
        pick(conn, rng, &categories())?
    } else {
        None
    };
    let payee = one_of(rng, &PAYEES);
    let data = InsertBankTransaction {
        bank_account_id: account.clone(),
        transaction_id: None,
        tx_type: one_of(rng, &["credit", "debit"]).to_string(),
        flow_kind: None,
        amount: money(rng),
        currency: Some(account_currency),
        description: Some(format!("Payment to {payee}")),
        counterparty_name: Some(payee.to_string()),
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        booking_date: day(rng),
        value_date: None,
        category_id: category.clone(),
        status: None,
    };
    run(
        format!("{account} {} {} {category:?}", data.tx_type, data.amount),
        || {
            data.validate()?;
            bank_accounts::create_transaction(conn, &data).map(drop)
        },
    )
}

fn transactions() -> String {
    by_content(
        "bank_transactions",
        "t.id",
        "",
        "t.booking_date, t.amount, t.tx_type, t.description",
    )
}

fn set_category(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(tx) = pick(conn, rng, &transactions())? else {
        return no_target();
    };
    let category = if rng.random_bool(0.8) {
        pick(conn, rng, &categories())?
    } else {
        None
    };
    run(format!("{tx} -> {category:?}"), || {
        bank_accounts::set_transaction_category(conn, &tx, category.as_deref(), None)
    })
}

fn record_balance(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some((account, account_currency)) = pick2(conn, rng, &accounts("t.id, t.currency"))? else {
        return no_target();
    };
    let data = InsertBalanceRecord {
        amount: money(rng),
        currency: account_currency,
        as_of: day(rng),
        confirm_unknown_as_of: true,
        source_batch_id: None,
    };
    run(
        format!("{account} {} on {}", data.amount, data.as_of),
        || {
            data.validate()?;
            bank_accounts::record_balance(conn, &account, &data).map(drop)
        },
    )
}

fn delete_balance_record(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content(
        "bank_balance_records",
        "t.id",
        "",
        "t.as_of, t.amount, t.currency",
    );
    let Some(id) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(id.clone(), || {
        bank_accounts::delete_balance_record(conn, &id).map(drop)
    })
}

/// A small statement (one to three payments) imported through the real CSV path, which writes
/// its `csv_import_batches` row.
fn import_statement(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(account) = pick(conn, rng, &accounts("t.id"))? else {
        return no_target();
    };
    let mut content = String::from("Date,Amount,Description\n");
    for _ in 0..rng.random_range(1..=3) {
        let date = chrono::DateTime::from_timestamp(day(rng), 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let sign = one_of(rng, &["-", ""]);
        content.push_str(&format!(
            "{date},{sign}{},{}\n",
            money(rng),
            one_of(rng, &PAYEES)
        ));
    }
    let file = format!("statement-{}.csv", rng.random_range(1..4));
    let config = CsvImportConfig {
        delimiter: ",".into(),
        date_column: "Date".into(),
        date_format: "%Y-%m-%d".into(),
        amount_column: Some("Amount".into()),
        description_columns: Some(vec!["Description".into()]),
        ..CsvImportConfig::default()
    };
    run(format!("{account} {file} {content:?}"), || {
        csv_import::import_transactions(conn, &account, &file, &content, &config, None).map(drop)
    })
}

fn delete_import_batch(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content(
        "csv_import_batches",
        "t.id",
        "",
        "t.file_name, t.date_from, t.date_to, t.imported_count",
    );
    let Some(id) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(id.clone(), || {
        csv_import::delete_import_batch(conn, &id).map(drop)
    })
}

fn learn_csv_format(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let (name, headers) = one_of(
        rng,
        &[
            ("Banka A", ["Datum", "Částka", "Poznámka"]),
            ("Banka B", ["Datum", "Částka", "Popis"]),
        ],
    );
    let data = InsertLearnedCsvFormat {
        name: name.to_string(),
        headers: headers.iter().map(|h| h.to_string()).collect(),
        delimiter: ";".into(),
        encoding: "UTF-8".into(),
        header_row: 0,
        mapping: LearnedCsvMapping {
            date_column: "Datum".into(),
            date_format: one_of(rng, &["%d.%m.%Y", "%Y-%m-%d"]).to_string(),
            amount_column: Some("Částka".into()),
            debit_column: None,
            credit_column: None,
            balance_column: None,
            transaction_id_column: None,
            description_columns: vec![headers[2].to_string()],
            counterparty_column: None,
            counterparty_iban_column: None,
            counterparty_bank_code_column: None,
            counterparty_country_column: None,
            currency_column: None,
            decimal_separator: ",".into(),
        },
        account_id: None,
    };
    run(format!("{name:?} {}", data.mapping.date_format), || {
        learned::save_format(conn, &data).map(drop)
    })
}

fn create_category(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let parent = if rng.random_bool(0.3) {
        pick(conn, rng, &categories())?
    } else {
        None
    };
    let data = InsertTransactionCategory {
        kind: None,
        name: one_of(rng, &CATEGORY_NAMES).to_string(),
        icon: Some("tag".into()),
        color: Some("#336699".into()),
        parent_id: parent.clone(),
        sort_order: None,
    };
    run(format!("{:?} under {parent:?}", data.name), || {
        data.validate()?;
        categories::create_category(conn, &data).map(drop)
    })
}

fn update_category(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &categories())? else {
        return no_target();
    };
    let data = UpdateTransactionCategory {
        kind: None,
        name: Some(one_of(rng, &CATEGORY_NAMES).to_string()),
        icon: Some(one_of(rng, &["tag", "cart", "plane"]).to_string()),
        color: Some(one_of(rng, &["#336699", "#993366"]).to_string()),
    };
    run(format!("{id} -> {:?}", data.name), || {
        data.validate()?;
        categories::update_category(conn, &id, &data).map(drop)
    })
}

/// The screen offers deleting the categories a person made; the system ones are refused.
fn delete_category(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let own = by_content(
        "transaction_categories",
        "t.id",
        "WHERE t.is_system = 0",
        "t.name, t.icon, t.color",
    );
    let Some(id) = pick(conn, rng, &own)? else {
        return no_target();
    };
    let reassign = if rng.random_bool(0.5) {
        pick(conn, rng, &categories())?.filter(|other| *other != id)
    } else {
        None
    };
    run(format!("{id} reassign to {reassign:?}"), || {
        categories::delete_category(conn, &id, reassign.as_deref())
    })
}

fn learn_scoped(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let (Some(account), Some(category)) = (
        pick(conn, rng, &accounts("t.id"))?,
        pick(conn, rng, &categories())?,
    ) else {
        return no_target();
    };
    let data = InsertScopedLearning {
        bank_account_id: account,
        is_credit: rng.random_bool(0.3),
        payee: Some(one_of(rng, &PAYEES).to_string()),
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        category_id: category,
    };
    run(
        format!(
            "{} {:?} -> {}",
            data.bank_account_id, data.payee, data.category_id
        ),
        || learn::learn_scoped(conn, &CategorizationEngine::new_empty(), &data),
    )
}

/// The synced profile columns (the Local ones, language, theme and menu, are left alone).
fn update_profile(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = UpdateUserProfile {
        name: rng
            .random_bool(0.5)
            .then(|| one_of(rng, &["Alex", "Sam"]).to_string()),
        surname: rng.random_bool(0.3).then(|| "Novák".to_string()),
        email: None,
        menu_preferences: None,
        currency: rng.random_bool(0.4).then(|| currency(rng).to_string()),
        language: None,
        exclude_personal_real_estate: rng.random_bool(0.3).then(|| rng.random_bool(0.5)),
        mcp_server_enabled: None,
        country: rng
            .random_bool(0.3)
            .then(|| one_of(rng, &["CZ", "DE"]).to_string()),
        insurance_notice_days: rng.random_bool(0.3).then(|| one_of(rng, &[30_i64, 43, 60])),
        theme: None,
    };
    run(format!("{data:?}"), || {
        auth::apply_profile_update(conn, &data)
    })
}
