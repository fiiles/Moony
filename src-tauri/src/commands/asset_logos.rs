//! Stock logo commands (spec 2026-11-14-asset-logos): thin wrappers over
//! `services::asset_logos` (the cache) and `price_api::refresh_asset_logos` (the Yahoo fetch).
//! Neither takes a ticker: the backend builds the list from the holdings and the watchlist.

use tauri::State;

use crate::db::Database;
use crate::error::Result;
use crate::models::asset_logos::AssetLogo;
use crate::services::{asset_logos, price_api};

/// Every cached stock logo as a data URL.
#[tauri::command]
pub async fn get_asset_logos(db: State<'_, Database>) -> Result<Vec<AssetLogo>> {
    db.with_conn(asset_logos::cached)
}

/// Look up the logos of held and watched tickers that are due; returns how many were newly stored.
#[tauri::command]
pub async fn refresh_asset_logos(db: State<'_, Database>) -> Result<u32> {
    price_api::refresh_asset_logos(&db).await
}
