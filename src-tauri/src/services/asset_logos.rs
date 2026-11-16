//! Stock logos cached in the encrypted database (spec 2026-11-14-asset-logos, ADR 0031).
//!
//! The pure half of the feature: which tickers are due, how a Yahoo search answer yields a logo
//! URL, which Financial Modeling Prep URL serves a ticker's logo, how downloaded bytes become a
//! small PNG, and the `asset_logos` cache itself (a `Local` table: never synced). The network half
//! lives next to the other Yahoo search in `price_api::refresh_asset_logos`.
//!
//! Only tickers the backend reads from the holdings and the watchlist are looked up, so no name
//! the frontend passes (a loan, an insurer) can reach the network. Yahoo is asked first (it already
//! receives every ticker for prices; only `https://s.yimg.com/` URLs are accepted); a ticker Yahoo
//! has no logo for is asked of Financial Modeling Prep, which therefore learns those tickers.

use std::collections::HashMap;
use std::io::Cursor;

use base64::Engine;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;
use crate::models::asset_logos::AssetLogo;

/// A remembered miss is looked up again after 30 days; a found logo is kept for good.
pub const MISS_RETRY_SECS: i64 = 30 * 24 * 3600;
/// 2026-11-16 12:00 CET, after the last run of a Yahoo-only build (Financial Modeling Prep became
/// the second source in ADR 0031, merged 2026-11-15 evening; a Yahoo-only dev build still ran at
/// 20:09 that day). A miss recorded before it may be a Yahoo-only miss and is looked up again at
/// once; a released build never had the Yahoo-only lookup.
pub const SOURCES_CHANGED_AT: i64 = 1_794_826_800;
/// At most this many tickers are looked up in one run; the rest follow on the next run.
pub const MAX_PER_RUN: usize = 40;
/// A downloaded image larger than this is a miss (Yahoo's brand logos are a few KB).
pub const MAX_SOURCE_BYTES: usize = 512 * 1024;
/// Stored logos fit a `LOGO_SIZE`×`LOGO_SIZE` box.
pub const LOGO_SIZE: u32 = 96;
/// The only host a logo is downloaded from (Yahoo's CDN).
pub const LOGO_HOST: &str = "s.yimg.com";
/// Every accepted logo URL starts with this.
const LOGO_URL_PREFIX: &str = "https://s.yimg.com/";
/// Financial Modeling Prep's public logo images: `<prefix><TICKER>.png`, no API key.
const FMP_LOGO_PREFIX: &str = "https://financialmodelingprep.com/image-stock/";
/// A longer ticker is not asked of FMP.
const MAX_FMP_TICKER_LEN: usize = 20;
/// A longer `logoUrl` is not a logo URL.
const MAX_LOGO_URL_LEN: usize = 2048;
/// A source image claiming a larger side is not decoded (a decompression bomb, not a logo).
const MAX_SOURCE_SIDE: u32 = 4096;
/// A source image whose longer side is shorter than this is a favicon, not a logo: a miss.
const MIN_SOURCE_SIDE: u32 = 32;
/// Allocation cap while decoding one source image.
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;
const PNG_MIME: &str = "image/png";

/// Every stored logo as a data URL, ordered by ticker. Misses are not listed.
pub fn cached(conn: &Connection) -> Result<Vec<AssetLogo>> {
    let mut stmt = conn
        .prepare("SELECT ticker, image FROM asset_logos WHERE image IS NOT NULL ORDER BY ticker")?;
    let rows = stmt.query_map([], |row| {
        let ticker: String = row.get(0)?;
        let image: Vec<u8> = row.get(1)?;
        Ok(AssetLogo {
            ticker,
            data_url: format!(
                "data:{PNG_MIME};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(image)
            ),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The distinct upper-case tickers of every stock holding (closed positions included) and every
/// watched stock, ordered. Blank tickers are skipped.
pub fn tracked_tickers(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT UPPER(TRIM(ticker)) AS t FROM (
             SELECT ticker FROM stock_investments
             UNION ALL
             SELECT ticker FROM watched_stocks
         )
         WHERE TRIM(ticker) <> ''
         ORDER BY t",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The tickers (in the given order, upper case) that need a lookup: no row yet, a miss checked at
/// least [`MISS_RETRY_SECS`] ago, or a miss checked before [`SOURCES_CHANGED_AT`]. At most
/// [`MAX_PER_RUN`].
pub fn due(conn: &Connection, tickers: &[String], now: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT ticker, image IS NOT NULL, checked_at FROM asset_logos")?;
    let known: HashMap<String, (bool, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(tickers
        .iter()
        .map(|ticker| ticker.trim().to_uppercase())
        .filter(|ticker| match known.get(ticker) {
            None => true,
            Some((true, _)) => false,
            Some((false, checked_at)) => {
                *checked_at <= now - MISS_RETRY_SECS || *checked_at < SOURCES_CHANGED_AT
            }
        })
        .take(MAX_PER_RUN)
        .collect())
}

/// The logo URL Yahoo's search (`enableLogoUrl=true`) gives for exactly `ticker`.
///
/// Only the first quote whose `symbol` equals the ticker (ignoring case) counts: Yahoo may list
/// another listing of the same company first, whose logo is not this ticker's. The URL must be
/// `https` on [`LOGO_HOST`] with no port or credentials; anything else is `None` (a miss), and no
/// request is ever sent to it.
pub fn logo_url_from_search(body: &str, ticker: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    let ticker = ticker.trim();
    let quote = parsed.get("quotes")?.as_array()?.iter().find(|quote| {
        quote
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|symbol| symbol.trim().eq_ignore_ascii_case(ticker))
    })?;
    let url = quote.get("logoUrl")?.as_str()?;
    is_logo_url(url).then(|| url.to_string())
}

/// `https://s.yimg.com/…`, checked both as a plain prefix and on the parsed URL.
fn is_logo_url(url: &str) -> bool {
    if url.len() > MAX_LOGO_URL_LEN || !url.starts_with(LOGO_URL_PREFIX) {
        return false;
    }
    reqwest::Url::parse(url).is_ok_and(|parsed| {
        parsed.scheme() == "https"
            && parsed.host_str() == Some(LOGO_HOST)
            && parsed.port().is_none()
            && parsed.username().is_empty()
            && parsed.password().is_none()
    })
}

/// The Financial Modeling Prep logo URL of `ticker` (upper case), or `None` when the ticker is
/// not a plain symbol of letters, digits, `.` and `-` (an index such as `^GSPC`, a currency pair
/// such as `EURUSD=X`, or anything that could change the path): no request is sent then.
pub fn fmp_logo_url(ticker: &str) -> Option<String> {
    let ticker = ticker.trim().to_uppercase();
    let plain = !ticker.is_empty()
        && ticker.len() <= MAX_FMP_TICKER_LEN
        && ticker
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !ticker.starts_with('.')
        && !ticker.contains("..");
    plain.then(|| format!("{FMP_LOGO_PREFIX}{ticker}.png"))
}

/// Downloaded bytes as a PNG that fits [`LOGO_SIZE`]×[`LOGO_SIZE`] (never enlarged), or `None`
/// when they are not a PNG, JPEG or WebP image, do not decode, claim a side over
/// `MAX_SOURCE_SIDE`, have a longer side under `MIN_SOURCE_SIDE` or exceed [`MAX_SOURCE_BYTES`]. Re-encoding means the untrusted bytes
/// never reach the webview as they came.
pub fn normalize(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    if decoded.width().max(decoded.height()) < MIN_SOURCE_SIDE {
        return None;
    }
    let fitted = if decoded.width() > LOGO_SIZE || decoded.height() > LOGO_SIZE {
        decoded.resize(LOGO_SIZE, LOGO_SIZE, FilterType::Lanczos3)
    } else {
        decoded
    };
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(fitted.to_rgba8())
        .write_to(&mut out, ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// Record a lookup for `ticker` (stored upper case): `Some` is a normalized PNG, `None` a miss.
/// A miss never replaces a stored logo (two overlapping runs cannot erase one).
pub fn store(conn: &Connection, ticker: &str, image: Option<&[u8]>, now: i64) -> Result<()> {
    let ticker = ticker.trim().to_uppercase();
    let mime = image.map(|_| PNG_MIME);
    conn.execute(
        "INSERT INTO asset_logos (id, ticker, image, mime, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(ticker) DO UPDATE SET
             image = excluded.image,
             mime = excluded.mime,
             checked_at = excluded.checked_at
         WHERE excluded.image IS NOT NULL OR asset_logos.image IS NULL",
        params![Uuid::new_v4().to_string(), ticker, image, mime, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::io::Cursor;

    const NOW: i64 = 1_800_000_000;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE asset_logos (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                image BLOB,
                mime TEXT,
                checked_at INTEGER NOT NULL
            );
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE
            );
            CREATE TABLE watched_stocks (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn tickers(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("encode png");
        out.into_inner()
    }

    #[test]
    fn due_lists_new_tickers_and_old_misses_only() {
        let conn = setup_test_db();
        store(&conn, "AAPL", Some(&png(4, 4)), NOW - 10 * MISS_RETRY_SECS).unwrap();
        store(&conn, "MISS1", None, NOW - MISS_RETRY_SECS).unwrap();
        store(&conn, "MISS2", None, NOW - 60).unwrap();

        let due = due(&conn, &tickers(&["AAPL", "MISS1", "MISS2", "NEW"]), NOW).unwrap();

        assert_eq!(due, tickers(&["MISS1", "NEW"]));
    }

    #[test]
    fn due_rechecks_misses_recorded_before_the_sources_changed() {
        let conn = setup_test_db();
        // A miss from the Yahoo-only days is looked up again at once (FMP may have the logo)...
        store(&conn, "OLD_MISS", None, SOURCES_CHANGED_AT - 1).unwrap();
        // ...a miss recorded since then waits the usual 30 days.
        store(&conn, "NEW_MISS", None, SOURCES_CHANGED_AT + 60).unwrap();

        let due = due(
            &conn,
            &tickers(&["OLD_MISS", "NEW_MISS"]),
            SOURCES_CHANGED_AT + 120,
        )
        .unwrap();

        assert_eq!(due, tickers(&["OLD_MISS"]));
    }

    #[test]
    fn due_rechecks_the_last_yahoo_only_run() {
        // The owner's profile: a Yahoo-only build recorded TTWO and eight others as misses on
        // 2026-11-15 at 20:09 CET, after FMP was decided but before the FMP build ran.
        let conn = setup_test_db();
        let last_yahoo_only_run = 1_794_769_784;
        store(&conn, "TTWO", None, last_yahoo_only_run).unwrap();

        let due = due(&conn, &tickers(&["TTWO"]), last_yahoo_only_run + 15 * 3600).unwrap();

        assert_eq!(due, tickers(&["TTWO"]));
    }

    #[test]
    fn fmp_url_is_the_image_stock_path_of_the_ticker() {
        assert_eq!(
            fmp_logo_url("BRK-B").as_deref(),
            Some("https://financialmodelingprep.com/image-stock/BRK-B.png")
        );
        assert_eq!(
            fmp_logo_url(" cez.pr ").as_deref(),
            Some("https://financialmodelingprep.com/image-stock/CEZ.PR.png")
        );
        assert_eq!(
            fmp_logo_url("VWCE.DE").as_deref(),
            Some("https://financialmodelingprep.com/image-stock/VWCE.DE.png")
        );
    }

    #[test]
    fn fmp_url_refuses_tickers_that_are_not_plain_symbols() {
        assert_eq!(fmp_logo_url(""), None);
        assert_eq!(fmp_logo_url("   "), None);
        assert_eq!(fmp_logo_url("../x"), None);
        assert_eq!(fmp_logo_url("A/B"), None);
        assert_eq!(fmp_logo_url("A?B"), None);
        assert_eq!(fmp_logo_url("AAPL MSFT"), None);
        assert_eq!(fmp_logo_url(&"A".repeat(40)), None);
        // An index or a currency pair is not a company: no logo is asked for.
        assert_eq!(fmp_logo_url("^GSPC"), None);
        assert_eq!(fmp_logo_url("EURUSD=X"), None);
    }

    #[test]
    fn due_is_capped_at_max_per_run() {
        let conn = setup_test_db();
        let all: Vec<String> = (0..45).map(|i| format!("T{i}")).collect();

        let due = due(&conn, &all, NOW).unwrap();

        assert_eq!(due.len(), MAX_PER_RUN);
        assert_eq!(due, all[..MAX_PER_RUN].to_vec());
    }

    #[test]
    fn tracked_tickers_unions_holdings_and_watchlist_upper_case_distinct() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO stock_investments (id, ticker) VALUES ('s1', 'aapl'), ('s2', 'CEZ.PR'), ('s3', ' ');
             INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL'), ('w2', 'msft');",
        )
        .unwrap();

        let tracked = tracked_tickers(&conn).unwrap();

        assert_eq!(tracked, tickers(&["AAPL", "CEZ.PR", "MSFT"]));
    }

    #[test]
    fn search_takes_the_exact_symbol_only() {
        let other_first = r#"{"quotes":[
            {"symbol":"CEZ.F","logoUrl":"https://s.yimg.com/a.png"},
            {"symbol":"cez.pr"}
        ]}"#;
        assert_eq!(logo_url_from_search(other_first, "CEZ.PR"), None);

        let on_the_match = r#"{"quotes":[
            {"symbol":"CEZ.F","logoUrl":"https://s.yimg.com/a.png"},
            {"symbol":"cez.pr","logoUrl":"https://s.yimg.com/lb/brands/150x150_cez.png"}
        ]}"#;
        assert_eq!(
            logo_url_from_search(on_the_match, "CEZ.PR"),
            Some("https://s.yimg.com/lb/brands/150x150_cez.png".to_string())
        );
    }

    #[test]
    fn search_rejects_foreign_hosts_and_http() {
        let body =
            |url: &str| format!(r#"{{"quotes":[{{"symbol":"BTC-USD","logoUrl":"{url}"}}]}}"#);
        for url in [
            "https://s2.coinmarketcap.com/x.png",
            "http://s.yimg.com/x.png",
            "https://s.yimg.com.evil.com/x.png",
            "https://s.yimg.com@evil.com/x.png",
            "https://s.yimg.com:8443/x.png",
            "HTTPS://S.YIMG.COM/x.png",
            "",
        ] {
            assert_eq!(logo_url_from_search(&body(url), "BTC-USD"), None, "{url}");
        }
        assert_eq!(logo_url_from_search("{not json", "BTC-USD"), None);
        assert_eq!(logo_url_from_search(r#"{"quotes":[]}"#, "BTC-USD"), None);
        assert_eq!(logo_url_from_search(r#"{"news":[]}"#, "BTC-USD"), None);
    }

    #[test]
    fn normalize_resizes_to_96_png() {
        let large = normalize(&png(150, 150)).expect("a PNG normalizes");
        assert_eq!(
            image::guess_format(&large).unwrap(),
            image::ImageFormat::Png
        );
        let decoded = image::load_from_memory(&large).unwrap();
        assert_eq!(decoded.width(), LOGO_SIZE);
        assert!(decoded.height() <= LOGO_SIZE);

        let small = normalize(&png(40, 20)).expect("a small PNG normalizes");
        let decoded = image::load_from_memory(&small).unwrap();
        assert!(decoded.width().max(decoded.height()) <= LOGO_SIZE);

        let jpeg = {
            let img = image::RgbImage::from_pixel(200, 100, image::Rgb([1, 2, 3]));
            let mut out = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(img)
                .write_to(&mut out, image::ImageFormat::Jpeg)
                .unwrap();
            out.into_inner()
        };
        let from_jpeg = normalize(&jpeg).expect("a JPEG normalizes");
        assert_eq!(
            image::guess_format(&from_jpeg).unwrap(),
            image::ImageFormat::Png
        );
        let decoded = image::load_from_memory(&from_jpeg).unwrap();
        assert_eq!(decoded.width(), LOGO_SIZE);
        assert_eq!(decoded.height(), LOGO_SIZE / 2);

        let webp = {
            let img = image::RgbaImage::from_pixel(120, 120, image::Rgba([9, 8, 7, 255]));
            let mut out = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut out, image::ImageFormat::WebP)
                .unwrap();
            out.into_inner()
        };
        let from_webp = normalize(&webp).expect("a WebP normalizes");
        assert_eq!(
            image::load_from_memory(&from_webp).unwrap().width(),
            LOGO_SIZE
        );
    }

    #[test]
    fn normalize_rejects_garbage_gif_and_oversize() {
        assert_eq!(normalize(b"not an image"), None);
        assert_eq!(normalize(b""), None);

        let gif = {
            let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
            let mut out = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut out, image::ImageFormat::Gif)
                .unwrap();
            out.into_inner()
        };
        assert_eq!(normalize(&gif), None);

        let mut oversize = png(16, 16);
        oversize.resize(MAX_SOURCE_BYTES + 1, 0);
        assert_eq!(normalize(&oversize), None);

        let truncated = png(150, 150);
        assert_eq!(normalize(&truncated[..truncated.len() / 2]), None);
    }

    #[test]
    fn normalize_rejects_icons_too_small_to_show() {
        // FMP answers some tickers with a 16×16 favicon; enlarged it would only blur.
        assert_eq!(normalize(&png(16, 16)), None);
        assert_eq!(normalize(&png(31, 31)), None);
        // A wide wordmark (FMP's ASML is 128×35) is kept.
        assert!(normalize(&png(128, 35)).is_some());
        assert!(normalize(&png(32, 32)).is_some());
    }

    #[test]
    fn normalize_rejects_huge_dimensions() {
        // A tiny file that claims a 20000×20000 canvas must not be decoded.
        assert_eq!(normalize(&png(20_000, 1)), None);
    }

    #[test]
    fn store_and_cached_round_trip() {
        use base64::Engine;
        let conn = setup_test_db();
        let bytes = png(4, 4);

        store(&conn, "AAPL", Some(&bytes), NOW).unwrap();
        store(&conn, "NOLOGO", None, NOW).unwrap();

        let cached_now = cached(&conn).unwrap();
        assert_eq!(cached_now.len(), 1);
        assert_eq!(cached_now[0].ticker, "AAPL");
        assert_eq!(
            cached_now[0].data_url,
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            )
        );

        // A logo found later replaces the miss.
        store(&conn, "NOLOGO", Some(&bytes), NOW + 1).unwrap();
        let tickers: Vec<String> = cached(&conn)
            .unwrap()
            .into_iter()
            .map(|l| l.ticker)
            .collect();
        assert_eq!(tickers, vec!["AAPL".to_string(), "NOLOGO".to_string()]);
        let (count, mime, checked_at): (i64, Option<String>, i64) = conn
            .query_row(
                "SELECT COUNT(*), MAX(mime), MAX(checked_at) FROM asset_logos WHERE ticker = 'NOLOGO'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (count, mime.as_deref(), checked_at),
            (1, Some("image/png"), NOW + 1)
        );

        // A miss never erases a stored logo.
        store(&conn, "AAPL", None, NOW + 2).unwrap();
        let after = cached(&conn).unwrap();
        assert_eq!(after.len(), 2);
        assert_eq!(after[0], cached_now[0]);
    }

    #[test]
    fn store_keys_by_the_upper_case_ticker() {
        let conn = setup_test_db();
        store(&conn, "aapl", None, NOW).unwrap();
        store(&conn, "AAPL", Some(&png(4, 4)), NOW).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM asset_logos", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(cached(&conn).unwrap()[0].ticker, "AAPL");
    }
}
