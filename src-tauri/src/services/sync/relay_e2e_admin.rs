//! The admin side of the end-to-end suites: what the person who runs a Moony sync server does in
//! its admin page, driven through the admin API (`/admin/api`, Moony-relay-node `src/admin/api.ts`).
//! The suites never mint accounts on their own: a person is added here, and the connection code
//! the server answers is what the app's setup takes, as in real life.
//!
//! One `cargo test` run signs in once (a login is limited to 5 failures per client and 15
//! minutes): the session cookie is cached for the process. A non-GET call carries an `Origin`
//! whose host is the request's own, which the server checks.

use std::sync::OnceLock;

use reqwest::{header, redirect, Client, Method, Response, StatusCode};
use serde_json::{json, Value};

use super::connection_code::{parse_connection_code, ConnectionCode};

/// The environment variable of the server's `SETUP_CODE` (its fixed setup code for automated
/// installs). Without it the suites use `DEFAULT_SETUP_CODE`.
pub(super) const SETUP_CODE_VAR: &str = "MOONY_RELAY_E2E_SETUP_CODE";
const DEFAULT_SETUP_CODE: &str = "moony-e2e-setup-code";
const ADMIN_NAME: &str = "e2e-admin";
const ADMIN_PASSWORD: &str = "moony-e2e-admin-password";
const COOKIE: &str = "moony_admin";

/// The signed-in admin's cookie pair (`moony_admin=<token>`), one per process.
static SESSION: OnceLock<String> = OnceLock::new();

/// A person the admin added: the server's ID for them and the connection code it answered.
#[derive(Debug)]
pub(super) struct Person {
    pub(super) id: String,
    pub(super) code: ConnectionCode,
}

/// A signed-in admin session on the server at `base`.
pub(super) struct Admin {
    base: String,
    cookie: String,
}

/// The admin signed in on the server at `base` (`http://127.0.0.1:8795`): the first call of the
/// process sets the server up with its setup code (a fresh server) or signs in (a server an earlier
/// run set up), and makes sure the server knows its own address, so that the connection codes it
/// hands out name `base`.
pub(super) async fn admin(base: &str) -> Admin {
    let base = base.trim_end_matches('/').to_string();
    let cookie = match SESSION.get() {
        Some(cookie) => cookie.clone(),
        None => {
            let cookie = sign_in(&base).await;
            SESSION.get_or_init(|| cookie).clone()
        }
    };
    let admin = Admin { base, cookie };
    let settings = admin.call(Method::GET, "settings", None).await;
    let saved = admin.json(settings, StatusCode::OK).await;
    if saved["publicUrl"].is_null() {
        let body = json!({ "publicUrl": admin.base });
        let response = admin.call(Method::PUT, "settings", Some(body)).await;
        admin.json(response, StatusCode::OK).await;
    }
    admin
}

fn client() -> Client {
    Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .expect("admin client")
}

/// Sets the server up or signs in, and returns the session's cookie pair.
async fn sign_in(base: &str) -> String {
    let bare = Admin {
        base: base.to_string(),
        cookie: String::new(),
    };
    let state = bare.call(Method::GET, "state", None).await;
    let state = bare.json(state, StatusCode::OK).await;
    let credentials = json!({ "name": ADMIN_NAME, "password": ADMIN_PASSWORD });
    let (route, body) = if state["setupNeeded"] == json!(true) {
        let code = std::env::var(SETUP_CODE_VAR).unwrap_or_else(|_| DEFAULT_SETUP_CODE.into());
        let mut body = credentials;
        body["code"] = json!(code);
        ("setup", body)
    } else {
        ("login", credentials)
    };
    let response = bare.call(Method::POST, route, Some(body)).await;
    assert!(
        response.status() == StatusCode::NO_CONTENT,
        "the admin {route} answered {} (SETUP_CODE of the server = ${SETUP_CODE_VAR}, default \
         {DEFAULT_SETUP_CODE}; the admin of an earlier run needs the same password)",
        response.status()
    );
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|pair| pair.starts_with(&format!("{COOKIE}=")))
        .expect("the session cookie")
        .to_string()
}

impl Admin {
    async fn call(&self, method: Method, route: &str, body: Option<Value>) -> Response {
        let mut request = client()
            .request(method.clone(), format!("{}/admin/api/{route}", self.base))
            .header(header::COOKIE, &self.cookie);
        if method != Method::GET {
            request = request.header(header::ORIGIN, &self.base);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.expect("the admin call")
    }

    async fn json(&self, response: Response, expected: StatusCode) -> Value {
        let status = response.status();
        let text = response.text().await.expect("admin answer");
        assert_eq!(status, expected, "{text}");
        serde_json::from_str(&text).expect("admin answer is JSON")
    }

    /// Adds a person named `name`; the server answers their ID and connection code.
    pub(super) async fn add_person(&self, name: &str) -> Person {
        let response = self
            .call(Method::POST, "people", Some(json!({ "name": name })))
            .await;
        let answer = self.json(response, StatusCode::CREATED).await;
        Person {
            id: answer["person"]["id"].as_str().expect("id").to_string(),
            code: parse_connection_code(answer["connectionCode"].as_str().expect("code"))
                .expect("a connection code the app reads"),
        }
    }

    async fn set_state(&self, id: &str, state: &str) {
        let response = self
            .call(
                Method::PATCH,
                &format!("people/{id}"),
                Some(json!({ "state": state })),
            )
            .await;
        self.json(response, StatusCode::OK).await;
    }

    /// Blocks the person: their account and vaults stay, but their devices are refused.
    pub(super) async fn block(&self, id: &str) {
        self.set_state(id, "blocked").await;
    }

    pub(super) async fn unblock(&self, id: &str) {
        self.set_state(id, "active").await;
    }

    /// Deletes the person with their vaults.
    pub(super) async fn delete(&self, id: &str) {
        let response = self
            .call(Method::DELETE, &format!("people/{id}"), None)
            .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
