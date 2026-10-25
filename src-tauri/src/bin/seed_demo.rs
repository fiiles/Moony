//! Demo database generator
//!
//! Creates a fully working, encrypted Moony data directory seeded with
//! realistic synthetic finances — for screenshots, demos, and safe UI
//! development without touching real data.
//!
//! # Usage
//! ```bash
//! cd src-tauri
//! cargo run --bin seed_demo                  # writes <repo>/demo-home/…  (per-OS layout, see below)
//! cargo run --bin seed_demo -- --out <dir>   # writes the data directory to exactly <dir>
//! MOONY_SEED_OUT=<dir> cargo run --bin seed_demo
//! cargo run --bin seed_demo -- --legacy-attachments   # attachments as plain files, like 0.12
//! ```
//!
//! The investment flat gets a photo batch (two photos) and a PDF lease. By default they go into the
//! encrypted attachments store (`attachments.db`). `--legacy-attachments` writes them as plain
//! files with legacy rows, the way versions up to 0.12 did, so the conversion that runs after
//! unlock can be exercised.
//!
//! Without `--out` the data lands in `<repo>/demo-home/`, laid out like the home
//! directory the app would use, so it never touches your real profile:
//!
//! | OS      | Default data directory                                           |
//! |---------|------------------------------------------------------------------|
//! | macOS   | `demo-home/Library/Application Support/com.moonyfinance.desktop` |
//! | Linux   | `demo-home/.local/share/com.moonyfinance.desktop`                |
//! | Windows | `demo-home\AppData\Roaming\com.moonyfinance.desktop`             |
//!
//! On macOS and Linux run the app on it by redirecting `HOME` (the final path and
//! the exact command are printed). Windows resolves the data folder through the
//! shell, not the environment: copy the generated folder to
//! `%APPDATA%\com.moonyfinance.desktop` on a throwaway Windows user.
//!
//! `--out <dir>` is the data directory itself (it will hold `moony.db`, `salt`,
//! `key.enc`, `recovery.enc`). A non-empty `<dir>` is only cleared when an
//! earlier run of this tool created it (marker file `.seed_demo`), so a typo can
//! never wipe a real folder; a real profile directory is therefore refused.
//!
//! Unlock password: `demo1234`

use moony_lib::db::Database;
use moony_lib::models::{
    InsertBankAccount, InsertCryptoTransaction, InsertInvestmentTransaction, InsertUserProfile,
};
use moony_lib::services::{auth, bank_accounts, crypto_investments, investments};
use rusqlite::params;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const DEMO_PASSWORD: &str = "demo1234";

/// (qty, price, days_ago)
type Buys = &'static [(f64, f64, i64)];
const DAY: i64 = 86_400;

/// Deterministic pseudo-random generator (LCG) so every run produces the same data
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / (u32::MAX as f64)
    }
    /// uniform in [lo, hi)
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// Tauri bundle identifier; the app stores its data in a folder with this name.
const APP_IDENTIFIER: &str = "com.moonyfinance.desktop";
/// Written into a directory this tool created, so a re-run may clear it again.
const MARKER_FILE: &str = ".seed_demo";
const OUT_ENV: &str = "MOONY_SEED_OUT";

const USAGE: &str = "Usage: seed_demo [--out <dir>] [--legacy-attachments]

  --out <dir>             write the data directory to exactly <dir> (also: env MOONY_SEED_OUT)
  --legacy-attachments    store the demo attachments as plain files, like versions up to 0.12
  -h, --help              show this help

Without --out the demo data goes to <repo>/demo-home/<per-OS app data path>.";

#[derive(Debug, PartialEq)]
enum Cli {
    Help,
    Run {
        out: Option<PathBuf>,
        legacy_attachments: bool,
    },
}

/// `--out <dir>` / `--out=<dir>` / `-h`, with `MOONY_SEED_OUT` as the fallback
/// (the flag wins). Anything else is an error.
fn parse_args<I>(args: I, env_out: Option<String>) -> Result<Cli, String>
where
    I: IntoIterator<Item = String>,
{
    let mut out: Option<PathBuf> = None;
    let mut legacy_attachments = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Cli::Help),
            "--legacy-attachments" => legacy_attachments = true,
            "--out" => {
                let value = args.next().ok_or("--out needs a directory")?;
                out = Some(PathBuf::from(value));
            }
            _ => match arg.strip_prefix("--out=") {
                Some(value) => out = Some(PathBuf::from(value)),
                None => return Err(format!("unknown argument: {arg}")),
            },
        }
    }
    if out.as_ref().is_some_and(|p| p.as_os_str().is_empty()) {
        return Err("--out needs a non-empty directory".into());
    }
    let out = out.or_else(|| env_out.filter(|v| !v.is_empty()).map(PathBuf::from));
    Ok(Cli::Run {
        out,
        legacy_attachments,
    })
}

/// Where Tauri's `app_data_dir` lands under a home directory, per OS.
fn app_data_subpath(os: &str) -> PathBuf {
    match os {
        "macos" => ["Library", "Application Support", APP_IDENTIFIER]
            .iter()
            .collect(),
        "windows" => ["AppData", "Roaming", APP_IDENTIFIER].iter().collect(),
        _ => [".local", "share", APP_IDENTIFIER].iter().collect(),
    }
}

/// Make `dir` ready to receive a fresh profile.
///
/// The default location inside `demo-home/` is always cleared (it only ever
/// holds demo data). An explicit `--out` directory is created if missing, used
/// if empty, cleared only if a previous run marked it, and refused otherwise.
fn prepare_data_dir(dir: &Path, explicit: bool) -> Result<(), String> {
    if dir.exists() {
        let is_empty = std::fs::read_dir(dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .next()
            .is_none();
        let ours = dir.join(MARKER_FILE).exists();
        if explicit && !is_empty && !ours {
            return Err(format!(
                "{} is not empty and was not created by seed_demo; refusing to clear it. \
                 Choose an empty or new directory.",
                dir.display()
            ));
        }
        if !is_empty {
            std::fs::remove_dir_all(dir)
                .map_err(|e| format!("cannot clear {}: {e}", dir.display()))?;
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

fn main() {
    let cli = match parse_args(std::env::args().skip(1), std::env::var(OUT_ENV).ok()) {
        Ok(cli) => cli,
        Err(msg) => {
            eprintln!("seed_demo: {msg}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let (out, legacy_attachments) = match cli {
        Cli::Help => {
            println!("{USAGE}");
            return;
        }
        Cli::Run {
            out,
            legacy_attachments,
        } => (out, legacy_attachments),
    };

    // CARGO_MANIFEST_DIR is src-tauri, so this works from any working directory.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    let demo_home = repo_root.join("demo-home");
    let (data_dir, explicit) = match out {
        Some(dir) => (dir, true),
        None => (
            demo_home.join(app_data_subpath(std::env::consts::OS)),
            false,
        ),
    };

    if let Err(msg) = prepare_data_dir(&data_dir, explicit) {
        eprintln!("seed_demo: {msg}");
        std::process::exit(1);
    }

    let db_path = data_dir.join("moony.db");
    let db = Database::new();

    let profile = InsertUserProfile {
        name: "Alex".into(),
        surname: "Novák".into(),
        email: "demo@moony.app".into(),
        menu_preferences: None,
        currency: Some("CZK".into()),
        language: Some("en".into()),
        exclude_personal_real_estate: Some(false),
    };
    let recovery_key = auth::setup_account(&db, db_path.clone(), profile, DEMO_PASSWORD)
        .expect("setup demo account");

    let now = chrono::Utc::now().timestamp();
    let mut rng = Rng(20260806);

    seed_bank(&db, now, &mut rng);
    seed_stocks(&db, now);
    seed_crypto(&db, now);
    seed_static_assets(&db, now);
    seed_attachments(&db, &data_dir, now, legacy_attachments);
    seed_rate_history(&db, now, &mut rng);
    seed_portfolio_history(&db, now, &mut rng);

    verify(&db);

    std::fs::write(
        data_dir.join(MARKER_FILE),
        "Created by `cargo run --bin seed_demo`; safe to delete.\n",
    )
    .expect("write marker file");

    println!("\nDemo data directory: {}", data_dir.display());
    println!("Unlock password:     {DEMO_PASSWORD}");
    println!("Recovery key:        {recovery_key}");
    if explicit {
        println!(
            "\nThe app reads this folder when it is its app data directory \
             (macOS ~/Library/Application Support/{APP_IDENTIFIER}, \
             Linux ~/.local/share/{APP_IDENTIFIER}, Windows %APPDATA%\\{APP_IDENTIFIER})."
        );
    } else if cfg!(windows) {
        println!(
            "\nWindows reads %APPDATA%\\{APP_IDENTIFIER}: copy the folder above there \
             (use a throwaway Windows user, never your real profile)."
        );
    } else {
        // Tauri on Linux honours XDG_DATA_HOME over HOME; macOS ignores it.
        let xdg = if cfg!(target_os = "macos") {
            String::new()
        } else {
            format!(" XDG_DATA_HOME=\"{}/.local/share\"", demo_home.display())
        };
        println!("\nLaunch the app on demo data (from repo root):");
        println!(
            "  HOME=\"{}\"{xdg} RUSTUP_HOME=\"$HOME/.rustup\" CARGO_HOME=\"$HOME/.cargo\" npm run tauri dev",
            demo_home.display()
        );
    }
}

fn seed_bank(db: &Database, now: i64, rng: &mut Rng) {
    db.with_conn(|conn| {
        let fio = bank_accounts::create_account(
            conn,
            &InsertBankAccount {
                name: "Běžný účet".into(),
                account_type: Some("checking".into()),
                iban: None,
                bban: None,
                currency: Some("CZK".into()),
                balance: Some("84230.50".into()),
                balance_as_of: None,
                institution_id: Some("inst_fio".into()),
                interest_rate: None,
                has_zone_designation: None,
                termination_date: None,
                interest_rate_valid_until: None,
                exclude_from_balance: None,
            },
        )
        .expect("create fio account");

        let revolut = bank_accounts::create_account(
            conn,
            &InsertBankAccount {
                name: "Revolut".into(),
                account_type: Some("checking".into()),
                iban: None,
                bban: None,
                currency: Some("EUR".into()),
                balance: Some("2150.75".into()),
                balance_as_of: None,
                institution_id: Some("inst_revolut".into()),
                interest_rate: None,
                has_zone_designation: None,
                termination_date: None,
                interest_rate_valid_until: None,
                exclude_from_balance: None,
            },
        )
        .expect("create revolut account");

        let _savings = bank_accounts::create_account(
            conn,
            &InsertBankAccount {
                name: "Spořicí účet".into(),
                account_type: Some("savings".into()),
                iban: None,
                bban: None,
                currency: Some("CZK".into()),
                balance: Some("350000".into()),
                balance_as_of: None,
                institution_id: Some("inst_air_bank".into()),
                interest_rate: Some("4.5".into()),
                has_zone_designation: None,
                termination_date: None,
                interest_rate_valid_until: None,
                exclude_from_balance: None,
            },
        )
        .expect("create savings account");

        let tx = |account: &str,
                  tx_type: &str,
                  amount: f64,
                  currency: &str,
                  desc: &str,
                  counterparty: &str,
                  category: &str,
                  date: i64| {
            conn.execute(
                "INSERT INTO bank_transactions (
                    id, bank_account_id, tx_type, amount, currency, description,
                    counterparty_name, booking_date, category_id, status, data_source, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'booked', 'manual', ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    account,
                    tx_type,
                    format!("{amount:.2}"),
                    currency,
                    desc,
                    counterparty,
                    date,
                    category,
                ],
            )
            .expect("insert bank transaction");
        };

        // 4 months of activity on the CZK account
        for month in 0..4 {
            let month_start = now - (month * 30 + 28) * DAY;
            tx(
                &fio.id,
                "credit",
                68000.0,
                "CZK",
                "Mzda",
                "TechCorp s.r.o.",
                "cat_income",
                month_start,
            );
            tx(
                &fio.id,
                "debit",
                18500.0,
                "CZK",
                "Nájem",
                "Reality Plus",
                "cat_housing",
                month_start + DAY,
            );
            tx(
                &fio.id,
                "debit",
                3420.0,
                "CZK",
                "Elektřina a plyn",
                "ČEZ Prodej",
                "cat_utilities",
                month_start + 2 * DAY,
            );
            tx(
                &fio.id,
                "debit",
                24800.0,
                "CZK",
                "Splátka hypotéky",
                "Hypoteční banka",
                "cat_loan_payments",
                month_start + 3 * DAY,
            );
            for week in 0..4 {
                let d = month_start + (4 + week * 7) * DAY;
                tx(
                    &fio.id,
                    "debit",
                    rng.range(950.0, 1650.0),
                    "CZK",
                    "Nákup potravin",
                    "Albert",
                    "cat_groceries",
                    d,
                );
                if rng.next() > 0.4 {
                    tx(
                        &fio.id,
                        "debit",
                        rng.range(320.0, 890.0),
                        "CZK",
                        "Restaurace",
                        "Restaurace U Lípy",
                        "cat_dining",
                        d + 2 * DAY,
                    );
                }
                if rng.next() > 0.6 {
                    tx(
                        &fio.id,
                        "debit",
                        rng.range(500.0, 2400.0),
                        "CZK",
                        "Nákup",
                        "Alza.cz",
                        "cat_shopping",
                        d + 3 * DAY,
                    );
                }
            }
            tx(
                &fio.id,
                "debit",
                550.0,
                "CZK",
                "Lítačka",
                "DPP",
                "cat_transport",
                month_start + 5 * DAY,
            );
            tx(
                &fio.id,
                "debit",
                rng.range(400.0, 900.0),
                "CZK",
                "Kino a koncerty",
                "GoOut",
                "cat_entertainment",
                month_start + 12 * DAY,
            );

            // EUR account activity
            let d = month_start + 9 * DAY;
            tx(
                &revolut.id,
                "debit",
                rng.range(25.0, 80.0),
                "EUR",
                "Dinner abroad",
                "Bolt Food",
                "cat_dining",
                d,
            );
            if month % 2 == 0 {
                tx(
                    &revolut.id,
                    "debit",
                    rng.range(120.0, 420.0),
                    "EUR",
                    "Flight tickets",
                    "Ryanair",
                    "cat_travel",
                    d + 3 * DAY,
                );
            }
        }
        Ok(())
    })
    .expect("seed bank data");
}

fn seed_stocks(db: &Database, now: i64) {
    db.with_conn(|conn| {
        // (ticker, name, buys[(qty, price, days_ago)], current_price)
        let stocks: &[(&str, &str, Buys, f64)] = &[
            ("AAPL", "Apple Inc.", &[(15.0, 165.30, 310), (10.0, 195.80, 120)], 232.50),
            ("MSFT", "Microsoft Corporation", &[(8.0, 310.20, 280), (4.0, 390.50, 90)], 425.10),
            ("VOO", "Vanguard S&P 500 ETF", &[(10.0, 385.00, 400), (8.0, 445.60, 150)], 512.30),
            // Bought in pre-split units: NVDA split 10:1 on 2024-06-10, 900 days ago is before it.
            ("NVDA", "NVIDIA Corporation", &[(3.0, 950.0, 900), (20.0, 120.0, 200)], 178.90),
        ];

        for (ticker, name, buys, current) in stocks {
            let (first, rest) = buys.split_first().expect("at least one buy");
            let inv = investments::create_investment_with_transaction(
                conn,
                ticker,
                name,
                Some(&InsertInvestmentTransaction {
                    investment_id: None,
                    tx_type: "buy".into(),
                    ticker: ticker.to_string(),
                    company_name: name.to_string(),
                    quantity: first.0.to_string(),
                    price_per_unit: first.1.to_string(),
                    currency: "USD".into(),
                    transaction_date: now - first.2 * DAY,
                }),
            )
            .expect("create investment");

            for (qty, price, days_ago) in rest {
                investments::add_transaction_to_investment(
                    conn,
                    &inv.id,
                    &InsertInvestmentTransaction {
                        investment_id: Some(inv.id.clone()),
                        tx_type: "buy".into(),
                        ticker: ticker.to_string(),
                        company_name: name.to_string(),
                        quantity: qty.to_string(),
                        price_per_unit: price.to_string(),
                        currency: "USD".into(),
                        transaction_date: now - days_ago * DAY,
                    },
                )
                .expect("add investment transaction");
            }

            conn.execute(
                "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at)
                 VALUES (?1, ?2, ?3, 'USD', ?4, ?4)",
                params![Uuid::new_v4().to_string(), ticker, format!("{current:.2}"), now],
            )
            .expect("insert stock price");
        }
        Ok(())
    })
    .expect("seed stocks");
}

fn seed_crypto(db: &Database, now: i64) {
    db.with_conn(|conn| {
        let cryptos: &[(&str, &str, &str, Buys, f64)] = &[
            (
                "BTC",
                "BTC-USD",
                "Bitcoin",
                &[(0.25, 38500.0, 420), (0.17, 62000.0, 160)],
                97400.0,
            ),
            (
                "ETH",
                "ETH-USD",
                "Ethereum",
                &[(2.5, 2250.0, 380), (1.3, 3100.0, 130)],
                3650.0,
            ),
        ];

        for (ticker, symbol, name, buys, current) in cryptos {
            let (first, rest) = buys.split_first().expect("at least one buy");
            let inv = crypto_investments::create_crypto_with_transaction(
                conn,
                ticker,
                Some(symbol),
                name,
                Some(&InsertCryptoTransaction {
                    investment_id: None,
                    tx_type: "buy".into(),
                    ticker: ticker.to_string(),
                    name: name.to_string(),
                    quantity: first.0.to_string(),
                    price_per_unit: first.1.to_string(),
                    currency: "USD".into(),
                    transaction_date: now - first.2 * DAY,
                }),
            )
            .expect("create crypto");

            for (qty, price, days_ago) in rest {
                crypto_investments::add_transaction_to_crypto(
                    conn,
                    &inv.id,
                    &InsertCryptoTransaction {
                        investment_id: Some(inv.id.clone()),
                        tx_type: "buy".into(),
                        ticker: ticker.to_string(),
                        name: name.to_string(),
                        quantity: qty.to_string(),
                        price_per_unit: price.to_string(),
                        currency: "USD".into(),
                        transaction_date: now - days_ago * DAY,
                    },
                )
                .expect("add crypto transaction");
            }

            conn.execute(
                "INSERT INTO crypto_prices (id, symbol, price, currency, fetched_at)
                 VALUES (?1, ?2, ?3, 'USD', ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    ticker,
                    format!("{current:.2}"),
                    now
                ],
            )
            .expect("insert crypto price");
        }
        Ok(())
    })
    .expect("seed crypto");
}

fn seed_static_assets(db: &Database, now: i64) {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO real_estate (id, name, address, type, purchase_price, purchase_price_currency,
                market_price, market_price_currency, monthly_rent, monthly_rent_currency,
                recurring_costs, photos, notes, created_at, updated_at)
             VALUES (?1, 'Byt 3+kk Vinohrady', 'Korunní 89, Praha 3', 'personal', '6900000', 'CZK',
                '8400000', 'CZK', NULL, 'CZK', '[]', '[]', NULL, ?2, ?2)",
            params![Uuid::new_v4().to_string(), now - 900 * DAY],
        )
        .expect("insert personal real estate");
        conn.execute(
            "INSERT INTO real_estate (id, name, address, type, purchase_price, purchase_price_currency,
                market_price, market_price_currency, monthly_rent, monthly_rent_currency,
                recurring_costs, photos, notes, created_at, updated_at)
             VALUES (?1, 'Investiční byt 2+kk', 'Veveří 14, Brno', 'investment', '4150000', 'CZK',
                '4850000', 'CZK', '16500', 'CZK', '[]', '[]', NULL, ?2, ?2)",
            params![Uuid::new_v4().to_string(), now - 620 * DAY],
        )
        .expect("insert investment real estate");

        // `principal` is the original amount: amortized from `start_date` at this
        // payment it is about 4.32M today, which is where the loans history below
        // ends, and it is paid off just before `end_date`.
        conn.execute(
            "INSERT INTO loans (id, name, principal, currency, interest_rate, monthly_payment, start_date, end_date, created_at, updated_at)
             VALUES (?1, 'Hypotéka — Vinohrady', '4530000', 'CZK', '4.79', '24800', ?2, ?3, ?2, ?2)",
            params![Uuid::new_v4().to_string(), now - 900 * DAY, now + 25 * 365 * DAY],
        )
        .expect("insert loan");

        conn.execute(
            "INSERT INTO bonds (id, name, isin, coupon_value, interest_rate, maturity_date, currency, quantity, created_at, updated_at)
             VALUES (?1, 'Dluhopis Republiky 2029', 'CZ0001005888', '10000', '5.20', ?2, 'CZK', '15', ?3, ?3)",
            params![Uuid::new_v4().to_string(), now + 3 * 365 * DAY, now - 400 * DAY],
        )
        .expect("insert bond");

        for (cat, amount) in [
            ("cat_groceries", "6000"),
            ("cat_dining", "4000"),
            ("cat_entertainment", "2500"),
            ("cat_shopping", "5000"),
            ("cat_transport", "2000"),
        ] {
            conn.execute(
                "INSERT INTO budget_goals (id, category_id, timeframe, amount, currency)
                 VALUES (?1, ?2, 'monthly', ?3, 'CZK')",
                params![Uuid::new_v4().to_string(), cat, amount],
            )
            .expect("insert budget goal");
        }
        Ok(())
    })
    .expect("seed static assets");
}

/// A gradient JPEG with a coloured band, standing in for a photo of the flat.
fn demo_jpeg(top: [u8; 3], bottom: [u8; 3]) -> Vec<u8> {
    let (w, h) = (1600u32, 1000u32);
    let img = image::RgbImage::from_fn(w, h, |_, y| {
        let t = y as f32 / h as f32;
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        image::Rgb([
            mix(top[0], bottom[0]),
            mix(top[1], bottom[1]),
            mix(top[2], bottom[2]),
        ])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .expect("encode demo photo");
    out.into_inner()
}

/// A one-page PDF with a heading line, standing in for a scanned contract.
fn demo_pdf(title: &str) -> Vec<u8> {
    let content = format!("BT /F1 28 Tf 72 720 Td ({title}) Tj ET\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

/// Two photos and a PDF lease on the investment flat: in the encrypted store, or (with
/// `--legacy-attachments`) as plain files and legacy rows, the way versions up to 0.12 kept them.
fn seed_attachments(db: &Database, data_dir: &Path, now: i64, legacy: bool) {
    use moony_lib::services::{attachments, real_estate_photos};

    let photos = [
        demo_jpeg([62, 125, 160], [236, 201, 152]),
        demo_jpeg([88, 120, 84], [214, 222, 196]),
    ];
    let lease = demo_pdf("Najemni smlouva - Veveri 14");
    db.with_conn(|conn| {
        let property: String = conn.query_row(
            "SELECT id FROM real_estate WHERE name = 'Investiční byt 2+kk'",
            [],
            |r| r.get(0),
        )?;
        let batch = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date, description, created_at)
             VALUES (?1, ?2, ?3, 'Předání bytu nájemníkovi', ?3)",
            params![batch, property, now - 30 * DAY],
        )?;

        if legacy {
            let photo_dir = data_dir.join(real_estate_photos::PHOTOS_DIR).join(&property).join(&batch);
            std::fs::create_dir_all(&photo_dir).expect("photo dir");
            for bytes in &photos {
                let id = Uuid::new_v4().to_string();
                let thumb = image::load_from_memory(bytes).expect("decode").thumbnail(400, 400);
                let mut thumb_bytes = std::io::Cursor::new(Vec::new());
                thumb
                    .write_to(&mut thumb_bytes, image::ImageFormat::Jpeg)
                    .expect("encode thumb");
                std::fs::write(photo_dir.join(format!("{id}.jpg")), bytes).expect("photo");
                std::fs::write(photo_dir.join(format!("{id}_thumb.jpg")), thumb_bytes.into_inner())
                    .expect("thumb");
                let rel = format!("{}/{property}/{batch}", real_estate_photos::PHOTOS_DIR);
                conn.execute(
                    "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, batch, format!("{rel}/{id}.jpg"), format!("{rel}/{id}_thumb.jpg"), now],
                )?;
            }
            let doc_dir = data_dir.join(attachments::REAL_ESTATE_DOCUMENTS_DIR).join(&property);
            std::fs::create_dir_all(&doc_dir).expect("document dir");
            let id = Uuid::new_v4().to_string();
            let path = doc_dir.join(format!("{id}_Nájemní smlouva.pdf"));
            std::fs::write(&path, &lease).expect("lease");
            conn.execute(
                "INSERT INTO real_estate_documents (id, real_estate_id, name, description, file_path, file_type, file_size, uploaded_at)
                 VALUES (?1, ?2, 'Nájemní smlouva', 'Na dobu neurčitou', ?3, 'contract', ?4, ?5)",
                params![id, property, path.to_string_lossy(), lease.len() as i64, now],
            )?;
        } else {
            for bytes in &photos {
                let prepared = real_estate_photos::prepare_photo(bytes)?;
                real_estate_photos::record_photo(conn, &batch, &prepared, now)?;
            }
            let picked = std::env::temp_dir().join(format!("seed-demo-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&picked).expect("temp dir");
            let source = picked.join("Nájemní smlouva.pdf");
            std::fs::write(&source, &lease).expect("lease");
            let doc = attachments::NewDocument {
                name: "Nájemní smlouva",
                description: Some("Na dobu neurčitou"),
                file_type: "contract",
            };
            attachments::add_real_estate_document(conn, &property, &source, &doc, now)?;
            let _ = std::fs::remove_dir_all(&picked);
        }
        Ok(())
    })
    .expect("seed attachments");
}

fn seed_rate_history(db: &Database, now: i64, rng: &mut Rng) {
    db.with_conn(|conn| {
        for day in 0..180 {
            let ts = now - day * DAY;
            let date = ts - ts % DAY;
            let drift = (day as f64) / 180.0;
            // CZK per unit, stored as units per 1 EUR (the ECB's own base).
            let eur = 24.85 + 0.45 * (day as f64 / 29.0).sin() + rng.range(-0.05, 0.05) - 0.3 * drift;
            let usd = 23.10 + 0.55 * (day as f64 / 23.0).sin() + rng.range(-0.06, 0.06) + 0.4 * drift;
            for (cur, rate) in [("CZK", eur), ("USD", eur / usd)] {
                conn.execute(
                    "INSERT OR IGNORE INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
                    params![date, cur, rate],
                )
                .expect("insert rate history");
            }
        }
        // current in-DB rates used at unlock
        for (cur, rate) in [("CZK", 24.55f64), ("USD", 24.55f64 / 23.45)] {
            conn.execute(
                "INSERT OR REPLACE INTO exchange_rates (currency, rate, fetched_at) VALUES (?1, ?2, ?3)",
                params![cur, rate, now],
            )
            .expect("insert current rate");
        }
        Ok(())
    })
    .expect("seed rate history");
}

fn seed_portfolio_history(db: &Database, now: i64, rng: &mut Rng) {
    db.with_conn(|conn| {
        for day in (0..180).rev() {
            let ts = now - day * DAY;
            let progress = 1.0 - (day as f64) / 180.0; // 0 → oldest, 1 → today
            let growth = |from: f64, to: f64| from + (to - from) * progress;

            let savings = growth(392_000.0, 486_000.0) + rng.range(-4_000.0, 4_000.0);
            let investments =
                growth(298_000.0, 405_000.0) * (1.0 + 0.03 * (day as f64 / 17.0).sin());
            let crypto = growth(212_000.0, 268_000.0) * (1.0 + 0.09 * (day as f64 / 11.0).sin());
            let bonds = 150_000.0;
            let re_personal = growth(8_150_000.0, 8_400_000.0);
            let re_investment = growth(4_700_000.0, 4_850_000.0);
            let loans = growth(4_410_000.0, 4_320_000.0);

            let usd_rate = 23.45;
            let inv_by_cur = format!("{{\"USD\":{:.2}}}", investments / usd_rate);
            let crypto_by_cur = format!("{{\"USD\":{:.2}}}", crypto / usd_rate);
            let savings_by_cur = format!(
                "{{\"CZK\":{:.2},\"EUR\":{:.2}}}",
                savings - 52_000.0,
                52_000.0 / 24.55
            );

            conn.execute(
                "INSERT INTO portfolio_metrics_history
                 (id, recorded_at,
                  investments_by_currency, crypto_by_currency, savings_by_currency,
                  bonds_by_currency, real_estate_by_currency, real_estate_personal_by_currency,
                  loans_by_currency, other_assets_by_currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    Uuid::new_v4().to_string(),
                    ts,
                    inv_by_cur,
                    crypto_by_cur,
                    savings_by_cur,
                    format!("{{\"CZK\":{bonds:.2}}}"),
                    format!("{{\"CZK\":{:.2}}}", re_personal + re_investment),
                    format!("{{\"CZK\":{re_personal:.2}}}"),
                    format!("{{\"CZK\":{loans:.2}}}"),
                    "{}",
                ],
            )
            .expect("insert history row");
        }
        Ok(())
    })
    .expect("seed portfolio history");
}

fn verify(db: &Database) {
    db.with_conn(|conn| {
        println!("\nSeeded demo database contents:");
        for table in [
            "bank_accounts",
            "bank_transactions",
            "stock_investments",
            "investment_transactions",
            "crypto_investments",
            "crypto_transactions",
            "real_estate",
            "real_estate_photos",
            "real_estate_documents",
            "loans",
            "bonds",
            "budget_goals",
            "portfolio_metrics_history",
            "exchange_rate_history",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .expect("count");
            println!("  {table:<28} {count:>5} rows");
        }
        let avg = moony_lib::services::cost_basis::cost_basis_for_all(
            conn,
            moony_lib::services::cost_basis::TxTable::Stocks,
            "CZK",
            &moony_lib::services::fx::current(),
        )
        .expect("cost basis")
        .0
        .values()
        .find(|_| true)
        .map(|p| p.average_price_native())
        .unwrap_or(0.0);
        println!("  first position average price (derived): {avg}");
        Ok(())
    })
    .expect("verify seeded db");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_use_the_default_location() {
        assert_eq!(
            parse_args(args(&[]), None),
            Ok(Cli::Run {
                out: None,
                legacy_attachments: false
            })
        );
    }

    #[test]
    fn the_legacy_attachments_flag_combines_with_out() {
        assert_eq!(
            parse_args(args(&["--legacy-attachments", "--out", "/tmp/demo"]), None),
            Ok(Cli::Run {
                out: Some(PathBuf::from("/tmp/demo")),
                legacy_attachments: true,
            })
        );
    }

    #[test]
    fn out_flag_takes_a_directory_in_both_spellings() {
        let expected = Ok(Cli::Run {
            out: Some(PathBuf::from("/tmp/demo")),
            legacy_attachments: false,
        });
        assert_eq!(parse_args(args(&["--out", "/tmp/demo"]), None), expected);
        assert_eq!(parse_args(args(&["--out=/tmp/demo"]), None), expected);
    }

    #[test]
    fn environment_variable_is_the_fallback_and_the_flag_wins() {
        assert_eq!(
            parse_args(args(&[]), Some("/env/dir".into())),
            Ok(Cli::Run {
                out: Some(PathBuf::from("/env/dir")),
                legacy_attachments: false,
            })
        );
        assert_eq!(
            parse_args(args(&["--out", "/flag/dir"]), Some("/env/dir".into())),
            Ok(Cli::Run {
                out: Some(PathBuf::from("/flag/dir")),
                legacy_attachments: false,
            })
        );
        // An empty variable counts as unset.
        assert_eq!(
            parse_args(args(&[]), Some(String::new())),
            Ok(Cli::Run {
                out: None,
                legacy_attachments: false
            })
        );
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert!(parse_args(args(&["--out"]), None).is_err());
        assert!(parse_args(args(&["--out="]), None).is_err());
        assert!(parse_args(args(&["--bogus"]), None).is_err());
        assert!(parse_args(args(&["positional"]), None).is_err());
    }

    #[test]
    fn help_wins() {
        assert_eq!(parse_args(args(&["-h"]), None), Ok(Cli::Help));
        assert_eq!(
            parse_args(args(&["--out", "/x", "--help"]), None),
            Ok(Cli::Help)
        );
    }

    #[test]
    fn default_layout_matches_each_platforms_app_data_dir() {
        assert_eq!(
            app_data_subpath("macos"),
            Path::new("Library/Application Support/com.moonyfinance.desktop")
        );
        assert_eq!(
            app_data_subpath("linux"),
            Path::new(".local/share/com.moonyfinance.desktop")
        );
        assert_eq!(
            app_data_subpath("windows"),
            Path::new("AppData/Roaming/com.moonyfinance.desktop")
        );
    }

    #[test]
    fn explicit_out_creates_missing_and_accepts_empty_directories() {
        let root = tempfile::tempdir().unwrap();
        let fresh = root.path().join("a/b/demo");
        prepare_data_dir(&fresh, true).unwrap();
        assert!(fresh.is_dir());

        // Empty directory: used as is.
        prepare_data_dir(&fresh, true).unwrap();
    }

    #[test]
    fn explicit_out_refuses_a_non_empty_directory_it_did_not_create() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real-profile");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("moony.db"), b"precious").unwrap();

        let err = prepare_data_dir(&real, true).unwrap_err();
        assert!(err.contains("refusing"), "{err}");
        assert_eq!(std::fs::read(real.join("moony.db")).unwrap(), b"precious");
    }

    #[test]
    fn explicit_out_clears_a_directory_marked_by_a_previous_run() {
        let root = tempfile::tempdir().unwrap();
        let demo = root.path().join("demo");
        std::fs::create_dir_all(&demo).unwrap();
        std::fs::write(demo.join(MARKER_FILE), b"x").unwrap();
        std::fs::write(demo.join("moony.db"), b"old demo").unwrap();

        prepare_data_dir(&demo, true).unwrap();
        assert!(demo.is_dir());
        assert!(!demo.join("moony.db").exists());
    }

    #[test]
    fn default_location_is_always_cleared() {
        let root = tempfile::tempdir().unwrap();
        let demo = root.path().join("demo-home/data");
        std::fs::create_dir_all(&demo).unwrap();
        std::fs::write(demo.join("moony.db"), b"old demo").unwrap();

        prepare_data_dir(&demo, false).unwrap();
        assert!(demo.is_dir());
        assert!(!demo.join("moony.db").exists());
    }
}
