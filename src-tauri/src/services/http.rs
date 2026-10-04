//! Shared outbound HTTP client.
//!
//! Every Moony-owned HTTP call (ECB rates, Frankfurter history, CoinGecko) goes
//! through [`client`]: one pooled `reqwest::Client` with a bounded timeout and an
//! identifying `User-Agent`. A bare `reqwest::get` has NO timeout and can hang a
//! command forever on a dead network, and CoinGecko's CloudFront edge rejects
//! requests without a User-Agent. The Yahoo connector builds its own client
//! (see `price_api::yahoo_connector`) with the same 15 s bound.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Client;

/// Upper bound for connecting and for a whole request/response round trip.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// `Moony/<version>`, sent with every request.
pub const USER_AGENT: &str = concat!("Moony/", env!("CARGO_PKG_VERSION"));

fn build_client(timeout: Duration) -> reqwest::Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
}

/// The process-wide client. Cloning a `Client` is cheap, but sharing the one
/// instance also shares its connection pool.
pub fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        build_client(REQUEST_TIMEOUT)
            .expect("HTTP client with default TLS configuration must build")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn user_agent_names_the_app_and_its_version() {
        assert!(USER_AGENT.starts_with("Moony/"));
        assert_eq!(USER_AGENT, format!("Moony/{}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn timeout_is_bounded_to_fifteen_seconds() {
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(15));
    }

    #[test]
    fn client_is_a_singleton() {
        assert!(std::ptr::eq(client(), client()));
    }

    /// Accepts one connection, returns the raw request head it received.
    fn serve_once_capturing_request(listener: TcpListener) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).expect("read request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write response");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        })
    }

    #[tokio::test]
    async fn requests_carry_the_moony_user_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = serve_once_capturing_request(listener);

        let body = build_client(Duration::from_secs(5))
            .expect("client")
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .expect("response")
            .text()
            .await
            .expect("body");
        assert_eq!(body, "ok");

        let request = server.join().expect("server thread").to_lowercase();
        assert!(
            request.contains(&format!("user-agent: {}", USER_AGENT.to_lowercase())),
            "missing Moony user agent in request: {request}"
        );
    }

    #[tokio::test]
    async fn a_server_that_never_answers_times_out() {
        // Accepts the connection and then stays silent: without a timeout the
        // request would hang forever.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let _conn = listener.accept().expect("accept");
            let _ = hold.recv();
        });

        let err = build_client(Duration::from_millis(300))
            .expect("client")
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .expect_err("silent server must time out");
        assert!(err.is_timeout(), "expected a timeout, got: {err}");

        let _ = release.send(());
        server.join().expect("server thread");
    }
}
