// Inject this fragment immediately before the final closing brace of
// src-tauri/src/services/mcp/bank_accounts.rs (inside its existing tests module).
// Audit evidence tests intentionally assert current incorrect behavior.

    fn audit_add_planning_tables(conn: &Connection) {
        conn.execute_batch("CREATE TABLE loans(id TEXT,name TEXT,principal TEXT,currency TEXT,interest_rate TEXT,monthly_payment TEXT,start_date INTEGER,end_date INTEGER,balance_anchor_amount TEXT,balance_anchor_date INTEGER);
 CREATE TABLE cashflow_items(id TEXT PRIMARY KEY,name TEXT,amount TEXT,currency TEXT,frequency TEXT,item_type TEXT,category TEXT,created_at INTEGER,updated_at INTEGER,replacement_source_id TEXT UNIQUE);
 CREATE TABLE real_estate(id TEXT,name TEXT,type TEXT,recurring_costs TEXT,monthly_rent TEXT,monthly_rent_currency TEXT);
 CREATE TABLE real_estate_loans(real_estate_id TEXT,loan_id TEXT);
 CREATE TABLE real_estate_insurances(real_estate_id TEXT,insurance_id TEXT);
 CREATE TABLE insurance_policies(id TEXT,policy_name TEXT,regular_payment TEXT,regular_payment_currency TEXT,payment_frequency TEXT,status TEXT);
 CREATE TABLE stock_investments(ticker TEXT,company_name TEXT,quantity TEXT);
 CREATE TABLE bonds(id TEXT,name TEXT,coupon_value TEXT,quantity TEXT,interest_rate TEXT,currency TEXT);
 CREATE TABLE cashflow_plan_exclusions(key TEXT PRIMARY KEY,created_at INTEGER NOT NULL DEFAULT 0);").unwrap();
    }

    #[test]
    fn audit_mcp_savings_missing_optional_rate_breaks_whole_plan() {
        let conn = setup_test_db();
        audit_add_planning_tables(&conn);
        let report = || crate::services::cashflow_planning::get_report_in_currency(&conn,"monthly","CZK");
        assert!(report().is_ok(), "baseline fixture should produce a report");
        let mut args = account_args("MCP savings", None);
        args.account_type = Some("savings".into());
        let account = bank_account_create(&conn, &args).unwrap();
        let rate: Option<String> = conn.query_row("SELECT interest_rate FROM bank_accounts WHERE id=?1", [account["id"].as_str().unwrap()], |r|r.get(0)).unwrap();
        assert!(rate.is_none(), "omitted optional interestRate becomes SQL NULL");
        let error = report().unwrap_err();
        assert!(error.to_string().contains("Null"), "unexpected failure: {error}");
        conn.execute("UPDATE bank_accounts SET interest_rate='0' WHERE id=?1", [account["id"].as_str().unwrap()]).unwrap();
        assert!(report().is_ok(), "repairing only the nullable rate restores the whole plan");
    }

    #[test]
    fn audit_mcp_uppercase_savings_loses_planned_interest() {
        let conn = setup_test_db();
        audit_add_planning_tables(&conn);
        let mut args = account_args("MCP upper savings", None);
        args.account_type = Some("SAVINGS".into());
        args.balance = Some("1000".into());
        args.interest_rate = Some("5".into());
        let account = bank_account_create(&conn, &args).unwrap();
        assert_eq!(account["accountType"], "SAVINGS");
        let report = crate::services::cashflow_planning::get_report_in_currency(&conn,"monthly","CZK").unwrap();
        let income = report.investments.income.iter().find(|c|c.key=="interestIncome").unwrap();
        assert!(income.items.is_empty(), "accepted uppercase account is omitted from interest");
        conn.execute("UPDATE bank_accounts SET account_type='savings' WHERE id=?1", [account["id"].as_str().unwrap()]).unwrap();
        let repaired = crate::services::cashflow_planning::get_report_in_currency(&conn,"monthly","CZK").unwrap();
        let income = repaired.investments.income.iter().find(|c|c.key=="interestIncome").unwrap();
        assert_eq!(income.items.len(),1);
        assert!(income.items[0].amount.parse::<f64>().unwrap() > 0.0);
    }

    #[test]
    fn audit_mcp_persists_unrenderable_booking_and_value_dates() {
        let mut conn = setup_test_db();
        let mut value = row("debit","10",1700000000,Some("invalid value day"));
        value.value_date = Some(i64::MAX);
        let args = BankTransactionsCreateArgs {account_id:"acc-1".into(),file_name:None,transactions:vec![row("debit","20",i64::MAX,Some("invalid booking day")),value]};
        let report = bank_transactions_create(&mut conn,&args,None).unwrap();
        assert_eq!(report.created,2);
        assert!(report.errors.is_empty());
        let invalid: i64 = conn.query_row("SELECT COUNT(*) FROM bank_transactions WHERE booking_date=?1 OR value_date=?1",[i64::MAX],|r|r.get(0)).unwrap();
        assert_eq!(invalid,2);
        let batches=crate::services::csv_import::import_batches(&conn,"acc-1").unwrap();
        assert_eq!(batches[0].date_range.as_ref().unwrap().to,i64::MAX);
    }
