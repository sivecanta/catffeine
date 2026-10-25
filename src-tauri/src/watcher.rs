//! Local Agent / Process Watcher HTTP IPC Server.
//!
//! Exposes a minimal loopback HTTP server (`127.0.0.1:41905` by default, or `CATFFEINE_PORT`)
//! for external CLI tools, hooks, and AI agents:
//! - `POST /agent/start`: Increments active job counter, forces Keep-Awake ON, switches cat to `"working"`.
//! - `POST /agent/stop`: Decrements active job counter; when count hits 0, restores the user's sleep policy.
//! - `POST /window/show`: Unhides and brings the Catffeine window to the front.
//! - `POST /window/hide`: Hides the cat into mini-pill mode.
//! - `GET /status`: Returns the current [`AppStateSnapshot`] as JSON.

use std::time::Duration;

use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{apply_cat_visible_and_sync, sync_and_broadcast_state};

/// Default local port for the Catffeine agent watcher HTTP listener.
pub const DEFAULT_WATCHER_PORT: u16 = 41905;

/// Read timeout for incoming loopback HTTP requests.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Parsed route for the local watcher HTTP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherRoute {
    AgentStart,
    AgentStop,
    WindowShow,
    WindowHide,
    Status,
    Options,
    NotFound,
}

/// Resolves the watcher port from `CATFFEINE_PORT` or returns [`DEFAULT_WATCHER_PORT`].
pub fn resolve_watcher_port() -> u16 {
    std::env::var("CATFFEINE_PORT")
        .ok()
        .and_then(|v| parse_port(&v))
        .unwrap_or(DEFAULT_WATCHER_PORT)
}

pub fn parse_port(val: &str) -> Option<u16> {
    val.trim().parse::<u16>().ok().filter(|&p| p > 0)
}

/// Parses the first HTTP request line into a [`WatcherRoute`].
pub fn parse_http_request(raw: &str) -> WatcherRoute {
    let first_line = raw.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("");
    let path = raw_path.split('?').next().unwrap_or("");

    match (method, path) {
        ("POST", "/agent/start") => WatcherRoute::AgentStart,
        ("POST", "/agent/stop") => WatcherRoute::AgentStop,
        ("POST", "/window/show") => WatcherRoute::WindowShow,
        ("POST", "/window/hide") => WatcherRoute::WindowHide,
        ("GET", "/status") | ("GET", "/agent/status") => WatcherRoute::Status,
        ("OPTIONS", _) => WatcherRoute::Options,
        _ => WatcherRoute::NotFound,
    }
}

/// Formats a minimal HTTP/1.1 JSON response with CORS headers.
pub fn format_http_response(status_line: &str, body: &str) -> String {
    format!(
        "{status_line}\r\n\
         Content-Type: application/json\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {body}",
        body.len()
    )
}

/// Spawns the background local HTTP watcher server on `127.0.0.1`.
pub fn spawn_watcher_server(app: AppHandle, port: u16) {
    tauri::async_runtime::spawn(async move {
        let addr = format!("127.0.0.1:{port}");
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(err) => {
                eprintln!("[catffeine] Failed to bind watcher server on {addr}: {err}");
                return;
            }
        };

        loop {
            let Ok((mut stream, _peer)) = listener.accept().await else {
                continue;
            };

            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                // ponytail: 1KB request buffer with 2s timeout; only method + path are needed
                let mut buf = [0u8; 1024];
                let n = match tokio::time::timeout(REQUEST_READ_TIMEOUT, stream.read(&mut buf)).await
                {
                    Ok(Ok(n)) if n > 0 => n,
                    _ => return,
                };

                let req = String::from_utf8_lossy(&buf[..n]);
                let (status_line, body) = match parse_http_request(&req) {
                    WatcherRoute::AgentStart => {
                        let snapshot = sync_and_broadcast_state(&app_handle, |inner| {
                            inner.agent_start();
                        });
                        (
                            "HTTP/1.1 200 OK",
                            serde_json::to_string(&snapshot).unwrap_or_default(),
                        )
                    }
                    WatcherRoute::AgentStop => {
                        let snapshot = sync_and_broadcast_state(&app_handle, |inner| {
                            inner.agent_stop();
                        });
                        (
                            "HTTP/1.1 200 OK",
                            serde_json::to_string(&snapshot).unwrap_or_default(),
                        )
                    }
                    WatcherRoute::WindowShow => {
                        let snapshot = apply_cat_visible_and_sync(&app_handle, None, true);
                        (
                            "HTTP/1.1 200 OK",
                            serde_json::to_string(&snapshot).unwrap_or_default(),
                        )
                    }
                    WatcherRoute::WindowHide => {
                        let snapshot = apply_cat_visible_and_sync(&app_handle, None, false);
                        (
                            "HTTP/1.1 200 OK",
                            serde_json::to_string(&snapshot).unwrap_or_default(),
                        )
                    }
                    WatcherRoute::Status => {
                        let snapshot = sync_and_broadcast_state(&app_handle, |_| {});
                        (
                            "HTTP/1.1 200 OK",
                            serde_json::to_string(&snapshot).unwrap_or_default(),
                        )
                    }
                    WatcherRoute::Options => ("HTTP/1.1 204 No Content", String::new()),
                    WatcherRoute::NotFound => (
                        "HTTP/1.1 404 Not Found",
                        r#"{"error":"Use POST /agent/start, POST /agent/stop, POST /window/show, POST /window/hide, or GET /status"}"#
                            .to_string(),
                    ),
                };

                let response = format_http_response(status_line, &body);
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_watcher_routes() {
        assert_eq!(
            parse_http_request("POST /agent/start HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            WatcherRoute::AgentStart
        );
        assert_eq!(
            parse_http_request("POST /agent/stop?source=cli HTTP/1.1\r\n\r\n"),
            WatcherRoute::AgentStop
        );
        assert_eq!(
            parse_http_request("POST /window/show HTTP/1.1\r\n\r\n"),
            WatcherRoute::WindowShow
        );
        assert_eq!(
            parse_http_request("POST /window/hide HTTP/1.1\r\n\r\n"),
            WatcherRoute::WindowHide
        );
        assert_eq!(
            parse_http_request("GET /status HTTP/1.1\r\n\r\n"),
            WatcherRoute::Status
        );
        assert_eq!(
            parse_http_request("GET /agent/status HTTP/1.1\r\n\r\n"),
            WatcherRoute::Status
        );
        assert_eq!(
            parse_http_request("OPTIONS /agent/start HTTP/1.1\r\n\r\n"),
            WatcherRoute::Options
        );
    }

    #[test]
    fn rejects_unknown_or_malformed_requests() {
        assert_eq!(
            parse_http_request("GET /agent/start HTTP/1.1\r\n\r\n"),
            WatcherRoute::NotFound
        );
        assert_eq!(parse_http_request(""), WatcherRoute::NotFound);
    }

    #[test]
    fn parses_non_zero_ports_only() {
        assert_eq!(parse_port("41905"), Some(41905));
        assert_eq!(parse_port(" 8080 "), Some(8080));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("invalid"), None);
    }
}
