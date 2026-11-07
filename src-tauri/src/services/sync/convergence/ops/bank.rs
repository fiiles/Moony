//! Bank operations: accounts, payments, balance records, categories, learned payees.

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{currency, day, money, no_target, one_of, pick, pick2, run, OpFn, Outcome};
use crate::models::bank_accounts::{InsertBalanceRecord, InsertBankAccount};
use crate::models::bank_transactions::{
    InsertBankTransaction, InsertTransactionCategory, UpdateTransactionCategory,
};
use crate::models::categorization::InsertScopedLearning;
use crate::services::categorization::engine::CategorizationEngine;
use crate::services::categorization::learn;
use crate::services::{bank_accounts, categories};

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
    ("categories::create_category", create_category),
    ("categories::update_category", update_category),
    ("categories::delete_category", delete_category),
    ("categorization::learn::learn_scoped", learn_scoped),
];

const ACCOUNT_NAMES: [&str; 4] = ["Main", "Savings", "Travel", "Joint"];
const CATEGORY_NAMES: [&str; 6] = ["Food", "Travel", "Rent", "Fun", "Gifts", "Groceries"];
const PAYEES: [&str; 4] = ["Albert", "Lidl", "Shell", "Landlord"];
const ACCOUNTS: &str = "SELECT id FROM bank_accounts ORDER BY rowid";
const CATEGORIES: &str = "SELECT id FROM transaction_categories ORDER BY rowid";

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

fn create_account(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let account_currency = currency(rng);
    let data = account_data(rng, account_currency);
    run(format!("{:?} {:?}", data.name, data.currency), || {
        data.validate()?;
        bank_accounts::create_account(conn, &data).map(drop)
    })
}

fn update_account(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, ACCOUNTS) else {
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

fn delete_account(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, ACCOUNTS) else {
        return no_target();
    };
    run(id.clone(), || bank_accounts::delete_account(conn, &id))
}

fn create_transaction(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some((account, account_currency)) = pick2(
        conn,
        rng,
        "SELECT id, currency FROM bank_accounts ORDER BY rowid",
    ) else {
        return no_target();
    };
    let category = if rng.random_bool(0.5) {
        pick(conn, rng, CATEGORIES)
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

fn set_category(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(tx) = pick(conn, rng, "SELECT id FROM bank_transactions ORDER BY rowid") else {
        return no_target();
    };
    let category = if rng.random_bool(0.8) {
        pick(conn, rng, CATEGORIES)
    } else {
        None
    };
    run(format!("{tx} -> {category:?}"), || {
        bank_accounts::set_transaction_category(conn, &tx, category.as_deref(), None)
    })
}

fn record_balance(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some((account, account_currency)) = pick2(
        conn,
        rng,
        "SELECT id, currency FROM bank_accounts ORDER BY rowid",
    ) else {
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

fn delete_balance_record(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(
        conn,
        rng,
        "SELECT id FROM bank_balance_records ORDER BY rowid",
    ) else {
        return no_target();
    };
    run(id.clone(), || {
        bank_accounts::delete_balance_record(conn, &id).map(drop)
    })
}

fn create_category(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let parent = if rng.random_bool(0.3) {
        pick(conn, rng, CATEGORIES)
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

fn update_category(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, CATEGORIES) else {
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

fn delete_category(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, CATEGORIES) else {
        return no_target();
    };
    let reassign = if rng.random_bool(0.5) {
        pick(conn, rng, CATEGORIES).filter(|other| *other != id)
    } else {
        None
    };
    run(format!("{id} reassign to {reassign:?}"), || {
        categories::delete_category(conn, &id, reassign.as_deref())
    })
}

fn learn_scoped(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let (Some(account), Some(category)) = (pick(conn, rng, ACCOUNTS), pick(conn, rng, CATEGORIES))
    else {
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
