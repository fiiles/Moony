//! Device sync glue (spec §9.4): starts the engine when a sync-enabled profile unlocks and stops it
//! at lock, restore and account deletion. No commands yet (Task 19).

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::db::Database;
use crate::models::sync::SyncStatus;
use crate::services::sync::binding::{BindingNative, NoBinding};
use crate::services::sync::engine::runtime::{self, EngineHandle};
use crate::services::sync::engine::EngineEvents;
use crate::services::sync::merge::DATA_CHANGED_EVENT;
use crate::services::sync::relay::http::HttpRelay;
use crate::services::sync::{feature, keys, state};

/// The event that carries `SyncStatus` to the UI.
pub const STATUS_CHANGED_EVENT: &str = "sync-status-changed";

/// The running engine of the open profile, if any.
#[derive(Default)]
pub struct SyncEngineState(pub Mutex<Option<EngineHandle>>);

/// What the engine tells the app: the events, the history queue after a merge, the binding native
/// (Task 14 provides the platform's; until then none) and the profile's folder.
struct TauriEvents {
    app: AppHandle,
    db: Database,
    data_dir: PathBuf,
    native: NoBinding,
}

impl EngineEvents for TauriEvents {
    fn data_changed(&self) {
        if let Err(e) = self.app.emit(DATA_CHANGED_EVENT, ()) {
            log::warn!("[SYNC] Could not emit {DATA_CHANGED_EVENT}: {e}");
        }
        // A merge's repairs queue history recalculations (`repair`): run them.
        crate::commands::mcp_refresh::resume(self.app.clone(), self.db.clone());
    }

    fn status_changed(&self, status: &SyncStatus) {
        if let Err(e) = self.app.emit(STATUS_CHANGED_EVENT, status) {
            log::warn!("[SYNC] Could not emit {STATUS_CHANGED_EVENT}: {e}");
        }
    }

    fn binding(&self) -> &dyn BindingNative {
        &self.native
    }

    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
}

/// Starts the engine for the profile just unlocked. Nothing at all while the flag is off
/// (`feature::enabled`); a profile without sync costs one `SELECT` (G5). A running engine is
/// stopped first. Never fails: a problem is logged and the profile opens without sync.
pub(crate) fn start_engine(app: &AppHandle, db: &Database) {
    if !feature::enabled() {
        return;
    }
    stop_engine(app);
    let session = match db.current_session() {
        Ok(session) => session,
        Err(e) => {
            log::warn!("[SYNC] The engine did not start: {e}");
            return;
        }
    };
    let relay_url = session.with_conn(|conn| {
        if !state::load(conn)?.is_some_and(|sync| sync.enabled) {
            return Ok(None);
        }
        Ok(keys::account(conn)?.map(|(relay_url, _)| relay_url))
    });
    let relay_url = match relay_url {
        Ok(Some(url)) => url,
        Ok(None) => return,
        Err(e) => {
            log::warn!("[SYNC] The engine did not start: {e}");
            return;
        }
    };
    let relay = match HttpRelay::new(&relay_url) {
        Ok(relay) => relay,
        Err(_) => {
            log::warn!("[SYNC] The engine did not start: the stored relay address is not valid");
            return;
        }
    };
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("[SYNC] The engine did not start: no app data dir: {e}");
            return;
        }
    };
    let events = TauriEvents {
        app: app.clone(),
        db: session.clone(),
        data_dir,
        native: NoBinding,
    };
    // The engine's task goes on Tauri's runtime, whoever calls this.
    let _runtime = tauri::async_runtime::handle().inner().enter();
    let handle = runtime::spawn(relay, session, events);
    *app.state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
    log::info!("[SYNC] The sync engine started");
}

/// Stops the engine, if one runs (lock, restore, account deletion).
pub(crate) fn stop_engine(app: &AppHandle) {
    let handle = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(handle) = handle {
        handle.stop();
        log::info!("[SYNC] The sync engine stopped");
    }
}
