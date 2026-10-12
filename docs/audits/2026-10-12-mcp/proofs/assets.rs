// Append to scratch-copy src-tauri/src/services/mcp/mod.rs, run filter readonly_assets_audit.
#[cfg(test)]
mod readonly_assets_audit {
    use super::*;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(r#"
            CREATE TABLE user_profile (id INTEGER PRIMARY KEY, currency TEXT NOT NULL);
            INSERT INTO user_profile VALUES (1, 'EUR');
            CREATE TABLE other_assets (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, quantity TEXT NOT NULL DEFAULT '0',
                market_price TEXT NOT NULL DEFAULT '0', currency TEXT NOT NULL,
                average_purchase_price TEXT NOT NULL DEFAULT '0', yield_type TEXT NOT NULL DEFAULT 'none',
                yield_value TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
            );
            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, type TEXT NOT NULL,
                quantity TEXT NOT NULL, price_per_unit TEXT NOT NULL, currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL
            );
            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, value TEXT NOT NULL,
                currency TEXT NOT NULL, valued_at INTEGER NOT NULL, note TEXT, created_at INTEGER NOT NULL
            );
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, address TEXT NOT NULL, type TEXT NOT NULL,
                purchase_price TEXT NOT NULL, purchase_price_currency TEXT NOT NULL,
                market_price TEXT NOT NULL, market_price_currency TEXT NOT NULL,
                monthly_rent TEXT, monthly_rent_currency TEXT, recurring_costs TEXT,
                photos TEXT, notes TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                purchase_date INTEGER
            );
            CREATE TABLE real_estate_valuations (
                id TEXT PRIMARY KEY, real_estate_id TEXT NOT NULL, value TEXT NOT NULL,
                currency TEXT NOT NULL, valued_at INTEGER NOT NULL, note TEXT, created_at INTEGER NOT NULL
            );
            CREATE TABLE real_estate_one_time_costs (
                id TEXT PRIMARY KEY, real_estate_id TEXT, name TEXT, description TEXT,
                amount TEXT, currency TEXT, date INTEGER, created_at INTEGER
            );
            CREATE TABLE insurance_policies (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, provider TEXT NOT NULL, policy_name TEXT NOT NULL,
                policy_number TEXT NOT NULL, start_date INTEGER NOT NULL, end_date INTEGER,
                payment_frequency TEXT NOT NULL, one_time_payment TEXT, one_time_payment_currency TEXT,
                regular_payment TEXT NOT NULL, regular_payment_currency TEXT NOT NULL,
                limits TEXT, notes TEXT, status TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
            );
        "#).unwrap();
        conn
    }

    fn asset() -> crate::models::InsertOtherAsset {
        crate::models::InsertOtherAsset {
            name: "Gold".into(), quantity: None, market_price: Some("20".into()),
            currency: Some("EUR".into()), average_purchase_price: None,
            yield_type: Some("percent_purchase".into()), yield_value: Some("10".into()),
        }
    }

    fn row(currency: &str, price: &str) -> other_assets::OtherAssetTransactionRow {
        other_assets::OtherAssetTransactionRow {
            tx_type: "buy".into(), quantity: "1".into(), price_per_unit: price.into(),
            currency: currency.into(), transaction_date: 1_700_000_000,
        }
    }

    #[test]
    fn initial_transaction_leaves_zero_holding_despite_tool_recommending_unset_quantity() {
        let conn = db();
        let result = other_assets::other_asset_create(&conn, &other_assets::OtherAssetCreateArgs {
            asset: asset(), initial_transaction: Some(row("EUR", "10")),
        }).unwrap();
        assert_eq!(result["quantity"], "0");
        assert_eq!(result["averagePurchasePrice"], "0");
        let count: i64 = conn.query_row("SELECT count(*) FROM other_asset_transactions", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
        eprintln!("CONFIRMED: initial buy exists but holding quantity=0, cost=0, net-worth contribution=0");
    }

    #[test]
    fn mixed_currency_purchases_average_raw_numbers() {
        let mut conn = db();
        let result = other_assets::other_asset_create(&conn, &other_assets::OtherAssetCreateArgs {
            asset: asset(), initial_transaction: None,
        }).unwrap();
        let id = result["id"].as_str().unwrap().to_string();
        let report = other_assets::other_asset_transactions_create(&mut conn, &other_assets::OtherAssetTransactionsCreateArgs {
            asset_id: id.clone(), transactions: vec![row("EUR", "10"), row("CZK", "100")],
        }).unwrap();
        assert_eq!(report.created, 2);
        let (quantity, avg, currency): (String,String,String) = conn.query_row(
            "SELECT quantity, average_purchase_price, currency FROM other_assets WHERE id=?1", [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap();
        assert_eq!((quantity.as_str(), avg.as_str(), currency.as_str()), ("2", "55", "EUR"));
        eprintln!("CONFIRMED: 10 EUR + 100 CZK buys become averagePurchasePrice=55 EUR; at 25 CZK/EUR correct average is 7 EUR");
    }

    #[test]
    fn different_currency_same_numeric_transaction_is_dropped_as_duplicate() {
        let mut conn = db();
        let result = other_assets::other_asset_create(&conn, &other_assets::OtherAssetCreateArgs {
            asset: asset(), initial_transaction: None,
        }).unwrap();
        let report = other_assets::other_asset_transactions_create(&mut conn, &other_assets::OtherAssetTransactionsCreateArgs {
            asset_id: result["id"].as_str().unwrap().into(),
            transactions: vec![row("EUR", "10"), row("CZK", "10")],
        }).unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(report.skipped_duplicates.len(), 1);
        eprintln!("CONFIRMED: distinct EUR and CZK purchases collapse to one transaction");
    }

    #[test]
    fn real_estate_schema_advertised_house_type_is_persisted() {
        let conn = db();
        let result = real_estate::real_estate_create(&conn, &real_estate::RealEstateCreateArgs {
            name: "Own home".into(), address: "Main St 1".into(), property_type: "house".into(),
            purchase_price: Some("100000".into()), purchase_price_currency: Some("EUR".into()),
            purchase_date: None, market_price: Some("150000".into()), market_price_currency: Some("EUR".into()),
            monthly_rent: None, monthly_rent_currency: None, recurring_costs: None, notes: None,
        }).unwrap();
        assert_eq!(result["type"], "house");
        let kind: String = conn.query_row("SELECT type FROM real_estate WHERE id=?1", [result["id"].as_str().unwrap()], |r| r.get(0)).unwrap();
        assert_ne!(kind, "personal");
        assert_ne!(kind, "investment");
        eprintln!("CONFIRMED: advertised house type accepted; net_worth treats it as investment, cashflow_planning treats it as personal");
    }

    #[test]
    fn nan_quantity_passes_mcp_batch_validation_and_corrupts_holding() {
        let mut conn = db();
        let result = other_assets::other_asset_create(&conn, &other_assets::OtherAssetCreateArgs {
            asset: asset(), initial_transaction: None,
        }).unwrap();
        let mut bad = row("EUR", "10");
        bad.quantity = "NaN".into();
        let id = result["id"].as_str().unwrap().to_string();
        let report = other_assets::other_asset_transactions_create(&mut conn, &other_assets::OtherAssetTransactionsCreateArgs {
            asset_id: id.clone(), transactions: vec![bad],
        }).unwrap();
        assert_eq!(report.created, 1);
        assert!(report.errors.is_empty());
        let quantity: String = conn.query_row("SELECT quantity FROM other_assets WHERE id=?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(quantity, "NaN");
        eprintln!("CONFIRMED: NaN quantity imports successfully and persists as computed holding quantity");
    }

    fn policy() -> crate::models::InsertInsurancePolicy {
        crate::models::InsertInsurancePolicy {
            policy_type: "life".into(), provider: "Provider".into(), policy_name: "Family".into(),
            policy_number: Some("POL123".into()), start_date: 1_700_000_000, end_date: None,
            payment_frequency: "monthly".into(), one_time_payment: None, one_time_payment_currency: None,
            regular_payment: Some("100".into()), regular_payment_currency: Some("EUR".into()),
            limits: None, notes: None, status: None,
        }
    }

    #[test]
    fn insurance_retry_creates_duplicate_financial_record() {
        let conn = db();
        insurance::insurance_create(&conn, policy()).unwrap();
        insurance::insurance_create(&conn, policy()).unwrap();
        let count: i64 = conn.query_row("SELECT count(*) FROM insurance_policies WHERE policy_number='POL123'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
        eprintln!("CONFIRMED: identical insurance retry produces two policies and doubles active monthly premiums");
    }

    #[test]
    fn uppercase_active_policy_is_accepted_but_cashflow_query_omits_it() {
        let conn = db();
        let mut p = policy(); p.status = Some("ACTIVE".into());
        let result = insurance::insurance_create(&conn, p).unwrap();
        assert_eq!(result["status"], "ACTIVE");
        let active: i64 = conn.query_row("SELECT count(*) FROM insurance_policies WHERE status='active'", [], |r| r.get(0)).unwrap();
        assert_eq!(active, 0);
        eprintln!("CONFIRMED: ACTIVE accepted but canonical active cashflow predicate omits premium");
    }
}
