//! Embedded MCP server over Streamable HTTP.
//!
//! Starts when the user unlocks Moony with the MCP setting enabled (or flips
//! the setting on), on a fixed configurable port with a stable bearer token —
//! both stored in the encrypted database (services::mcp::config). Stops on
//! lock/disable. There is no session.json and no REST surface; the only
//! route is /mcp. See ADR 0006.

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use subtle::ConstantTimeEq;
use tokio::task::JoinHandle;

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::services::mcp::{config, MoonyMcp};

/// Does the `Authorization` header carry exactly `Bearer <expected>`?
///
/// The token bytes are compared with `subtle`'s constant-time equality so the
/// response time does not leak how many leading bytes of a guess were right.
/// Only the length (not secret: the token is a fixed-size random string) may
/// short-circuit the comparison.
fn bearer_matches(header: Option<&str>, expected: &str) -> bool {
    let Some(presented) = header.and_then(|v| v.strip_prefix("Bearer ")) else {
        return false;
    };
    presented.as_bytes().ct_eq(expected.as_bytes()).into()
}

async fn require_bearer(State(expected): State<Arc<String>>, req: Request, next: Next) -> Response {
    let header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    if bearer_matches(header, expected.as_str()) {
        next.run(req).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

pub struct LocalApiServer {
    handle: Mutex<Option<JoinHandle<()>>>,
    port: Mutex<Option<u16>>,
    last_error: Mutex<Option<String>>,
}

impl Default for LocalApiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalApiServer {
    pub fn new() -> Self {
        Self {
            handle: Mutex::new(None),
            port: Mutex::new(None),
            last_error: Mutex::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.handle.lock().unwrap().is_some()
    }

    pub fn get_port(&self) -> Option<u16> {
        *self.port.lock().unwrap()
    }

    pub fn get_last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    pub async fn start(&self, db: Database, app: tauri::AppHandle) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        let (token, port) = match db.with_conn(|conn| {
            let token = config::get_or_create_token(conn)?;
            let port = config::get_port(conn)?;
            Ok((token, port))
        }) {
            Ok(v) => v,
            Err(e) => {
                *self.last_error.lock().unwrap() = Some(e.to_string());
                return Err(e);
            }
        };

        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => {
                let msg = format!("Failed to bind 127.0.0.1:{}: {}", port, e);
                *self.last_error.lock().unwrap() = Some(msg.clone());
                return Err(AppError::Internal(msg));
            }
        };

        let mcp = MoonyMcp::new(db, app);
        let service = StreamableHttpService::new(
            move || Ok(mcp.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );
        let app =
            Router::new()
                .nest_service("/mcp", service)
                .layer(middleware::from_fn_with_state(
                    Arc::new(token),
                    require_bearer,
                ));

        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                log::error!("[local-api] Server error: {}", e);
            }
        });

        *self.handle.lock().unwrap() = Some(handle);
        *self.port.lock().unwrap() = Some(port);
        *self.last_error.lock().unwrap() = None;

        log::info!(
            "[local-api] MCP server started on http://127.0.0.1:{}/mcp",
            port
        );
        Ok(())
    }

    pub async fn stop(&self) {
        // Take the handle out in its own statement so the std Mutex guard is
        // dropped before the await below (holding it across .await makes this
        // future non-Send and the Tauri commands stop compiling).
        let handle = self.handle.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.abort();
            // Awaiting an aborted task returns promptly with a cancelled
            // JoinError; it guarantees the task's future — and the TcpListener
            // it owns — is dropped before we return, so a later start() can
            // rebind the same port.
            let _ = handle.await;
        }
        *self.port.lock().unwrap() = None;
        *self.last_error.lock().unwrap() = None;
        log::info!("[local-api] MCP server stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::bearer_matches;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn accepts_the_exact_bearer_token() {
        assert!(bearer_matches(Some(&format!("Bearer {TOKEN}")), TOKEN));
    }

    #[test]
    fn rejects_a_missing_header() {
        assert!(!bearer_matches(None, TOKEN));
    }

    #[test]
    fn rejects_a_header_without_the_bearer_scheme() {
        assert!(!bearer_matches(Some(TOKEN), TOKEN));
        assert!(!bearer_matches(Some(&format!("Basic {TOKEN}")), TOKEN));
        assert!(!bearer_matches(Some(&format!("bearer {TOKEN}")), TOKEN));
    }

    #[test]
    fn rejects_a_wrong_token_of_the_same_length() {
        let mut wrong = TOKEN.to_string();
        wrong.replace_range(31..32, "0");
        assert_eq!(wrong.len(), TOKEN.len());
        assert!(!bearer_matches(Some(&format!("Bearer {wrong}")), TOKEN));
        // Differs in the very first byte too.
        let first = format!("Bearer f{}", &TOKEN[1..]);
        assert!(!bearer_matches(Some(&first), TOKEN));
    }

    #[test]
    fn rejects_prefixes_and_extensions_of_the_token() {
        assert!(!bearer_matches(
            Some(&format!("Bearer {}", &TOKEN[..TOKEN.len() - 1])),
            TOKEN
        ));
        assert!(!bearer_matches(Some(&format!("Bearer {TOKEN}0")), TOKEN));
        assert!(!bearer_matches(Some("Bearer "), TOKEN));
        assert!(!bearer_matches(Some("Bearer"), TOKEN));
    }

    #[test]
    fn an_empty_expected_token_never_authenticates_a_non_empty_guess() {
        assert!(!bearer_matches(Some("Bearer x"), ""));
    }
}
