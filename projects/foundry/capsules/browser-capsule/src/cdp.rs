use crate::{Error, Input, Result, PAGE_LOAD_TIMEOUT};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

const NAVIGATE_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Cdp {
    socket: Mutex<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    next_id: AtomicU64,
}

impl Cdp {
    pub async fn connect(port: u16) -> Result<Self> {
        let url = page_websocket_url(port).await?;
        let (socket, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|error| Error::Backend(format!("CDP connect: {error}")))?;
        Ok(Self {
            socket: Mutex::new(socket),
            next_id: AtomicU64::new(1),
        })
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        let deadline = Instant::now() + NAVIGATE_TIMEOUT;
        loop {
            let result = self.send("Page.navigate", json!({ "url": url })).await?;
            if let Some(error) = result.get("errorText").and_then(Value::as_str) {
                if is_transient_navigation_error(error) && Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                return Err(Error::Backend(format!("navigate {url}: {error}")));
            }
            return self.wait_ready().await;
        }
    }

    async fn wait_ready(&self) -> Result<()> {
        let deadline = Instant::now() + PAGE_LOAD_TIMEOUT;
        loop {
            let result = self
                .send(
                    "Runtime.evaluate",
                    json!({ "expression": "document.readyState", "returnByValue": true }),
                )
                .await?;
            let state = result
                .pointer("/result/value")
                .and_then(Value::as_str)
                .unwrap_or("");
            if state == "complete" {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::NotReady(format!(
                    "page did not finish loading (readyState: {state})"
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        let result = self
            .send(
                "Page.captureScreenshot",
                json!({ "format": "png", "fromSurface": true }),
            )
            .await?;
        let data = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Backend("CDP screenshot missing data".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| Error::Backend(format!("decode screenshot: {error}")))
    }

    pub async fn input(&self, input: Input) -> Result<()> {
        match input {
            Input::Move { x, y } => self.mouse("mouseMoved", x, y, None, 0).await,
            Input::Click { x, y, button } => {
                let (name, bit) = button_name(button).ok_or_else(|| {
                    Error::InvalidRequest(format!("unsupported mouse button {button}"))
                })?;
                self.mouse("mouseMoved", x, y, Some(name), 0).await?;
                self.mouse("mousePressed", x, y, Some(name), bit).await?;
                self.mouse("mouseReleased", x, y, Some(name), 0).await
            }
            Input::Scroll { x, y, delta } => {
                self.send(
                    "Input.dispatchMouseEvent",
                    json!({
                        "type": "mouseWheel",
                        "x": x,
                        "y": y,
                        "deltaX": 0,
                        "deltaY": delta * 100,
                    }),
                )
                .await?;
                Ok(())
            }
            Input::Key { key } => self.key(&key).await,
            Input::Text { text } => {
                self.send("Input.insertText", json!({ "text": text }))
                    .await?;
                Ok(())
            }
        }
    }

    async fn mouse(
        &self,
        event: &str,
        x: i32,
        y: i32,
        button: Option<&str>,
        buttons: u8,
    ) -> Result<()> {
        let mut params = json!({
            "type": event,
            "x": x,
            "y": y,
            "buttons": buttons,
        });
        if let Some(button) = button {
            params["button"] = json!(button);
            params["clickCount"] = json!(1);
        }
        self.send("Input.dispatchMouseEvent", params).await?;
        Ok(())
    }

    async fn key(&self, key: &str) -> Result<()> {
        if key.chars().count() == 1 {
            self.send("Input.insertText", json!({ "text": key }))
                .await?;
            return Ok(());
        }
        let (key_name, code, vk) =
            key_def(key).ok_or_else(|| Error::InvalidRequest(format!("unsupported key {key}")))?;
        for event in ["keyDown", "keyUp"] {
            self.send(
                "Input.dispatchKeyEvent",
                json!({
                    "type": event,
                    "key": key_name,
                    "code": code,
                    "windowsVirtualKeyCode": vk,
                    "nativeVirtualKeyCode": vk,
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn send(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({ "id": id, "method": method, "params": params }).to_string();
        let mut socket = self.socket.lock().await;
        socket
            .send(Message::text(payload))
            .await
            .map_err(|error| Error::Backend(format!("CDP send: {error}")))?;
        loop {
            let message = socket
                .next()
                .await
                .ok_or_else(|| Error::Backend("CDP socket closed".into()))?
                .map_err(|error| Error::Backend(format!("CDP receive: {error}")))?;
            match message {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(text.as_str())
                        .map_err(|error| Error::Backend(format!("CDP JSON: {error}")))?;
                    if value.get("id").and_then(Value::as_u64) == Some(id) {
                        if let Some(error) = value.get("error") {
                            return Err(Error::Backend(format!("CDP {method}: {error}")));
                        }
                        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                    }
                }
                Message::Ping(payload) => {
                    socket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| Error::Backend(format!("CDP pong: {error}")))?;
                }
                Message::Close(_) => {
                    return Err(Error::Backend("CDP socket closed".into()));
                }
                _ => {}
            }
        }
    }
}

async fn page_websocket_url(port: u16) -> Result<String> {
    let reply: Value = reqwest::get(format!("http://127.0.0.1:{port}/json/list"))
        .await
        .map_err(|error| Error::Backend(format!("CDP target list: {error}")))?
        .json()
        .await
        .map_err(|error| Error::Backend(format!("CDP target list JSON: {error}")))?;
    if let Some(targets) = reply.as_array() {
        for target in targets {
            if target.get("type").and_then(Value::as_str) == Some("page") {
                if let Some(url) = target.get("webSocketDebuggerUrl").and_then(Value::as_str) {
                    return Ok(rewrite_ws_url(url, port));
                }
            }
        }
    }
    Err(Error::NotReady("CDP has no page target".into()))
}

/// Chromium reports its internal debugging port (9222). Over a Podman port
/// mapping the host port is different, so keep only the path and point it at
/// the published 127.0.0.1 port.
fn rewrite_ws_url(url: &str, port: u16) -> String {
    if let Some((_, rest)) = url.split_once("://") {
        if let Some(index) = rest.find('/') {
            return format!("ws://127.0.0.1:{port}{}", &rest[index..]);
        }
    }
    format!("ws://127.0.0.1:{port}/")
}

fn is_transient_navigation_error(error: &str) -> bool {
    [
        "ERR_NETWORK_CHANGED",
        "ERR_INTERNET_DISCONNECTED",
        "ERR_NAME_NOT_RESOLVED",
        "ERR_TIMED_OUT",
    ]
    .iter()
    .any(|candidate| error.contains(candidate))
}

fn button_name(button: u8) -> Option<(&'static str, u8)> {
    match button {
        1 => Some(("left", 1)),
        2 => Some(("middle", 4)),
        3 => Some(("right", 2)),
        _ => None,
    }
}

fn key_def(key: &str) -> Option<(&'static str, &'static str, i64)> {
    let value = match key {
        "Return" | "Enter" => ("Enter", "Enter", 13),
        "Tab" => ("Tab", "Tab", 9),
        "Backspace" => ("Backspace", "Backspace", 8),
        "Escape" | "Esc" => ("Escape", "Escape", 27),
        "Delete" => ("Delete", "Delete", 46),
        "Home" => ("Home", "Home", 36),
        "End" => ("End", "End", 35),
        "PageUp" | "Page_Up" => ("PageUp", "PageUp", 33),
        "PageDown" | "Page_Down" => ("PageDown", "PageDown", 34),
        "Left" | "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37),
        "Up" | "ArrowUp" => ("ArrowUp", "ArrowUp", 38),
        "Right" | "ArrowRight" => ("ArrowRight", "ArrowRight", 39),
        "Down" | "ArrowDown" => ("ArrowDown", "ArrowDown", 40),
        "Space" => (" ", "Space", 32),
        "F1" => ("F1", "F1", 112),
        "F2" => ("F2", "F2", 113),
        "F3" => ("F3", "F3", 114),
        "F4" => ("F4", "F4", 115),
        "F5" => ("F5", "F5", 116),
        "F6" => ("F6", "F6", 117),
        "F7" => ("F7", "F7", 118),
        "F8" => ("F8", "F8", 119),
        "F9" => ("F9", "F9", 120),
        "F10" => ("F10", "F10", 121),
        "F11" => ("F11", "F11", 122),
        "F12" => ("F12", "F12", 123),
        _ => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_mouse_buttons() {
        assert_eq!(button_name(1), Some(("left", 1)));
        assert_eq!(button_name(3), Some(("right", 2)));
        assert_eq!(button_name(9), None);
    }

    #[test]
    fn maps_named_keys() {
        assert_eq!(key_def("Return"), Some(("Enter", "Enter", 13)));
        assert_eq!(key_def("ArrowLeft"), Some(("ArrowLeft", "ArrowLeft", 37)));
        assert_eq!(key_def("F12"), Some(("F12", "F12", 123)));
        assert_eq!(key_def("NotAKey"), None);
    }

    #[test]
    fn recognizes_transient_navigation_errors() {
        assert!(is_transient_navigation_error("net::ERR_NETWORK_CHANGED"));
        assert!(is_transient_navigation_error("net::ERR_NAME_NOT_RESOLVED"));
        assert!(!is_transient_navigation_error("net::ERR_ABORTED"));
    }

    #[test]
    fn rewrites_container_cdp_url_to_host_port() {
        assert_eq!(
            rewrite_ws_url("ws://127.0.0.1:9222/devtools/page/abc", 45000),
            "ws://127.0.0.1:45000/devtools/page/abc"
        );
        assert_eq!(
            rewrite_ws_url("ws://localhost:9222/devtools/browser/xyz", 45001),
            "ws://127.0.0.1:45001/devtools/browser/xyz"
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn live_navigate_and_screenshot() {
        let port: u16 = std::env::var("BROWSER_CAPSULE_TEST_CDP_PORT")
            .expect("BROWSER_CAPSULE_TEST_CDP_PORT")
            .parse()
            .expect("port");
        let cdp = Cdp::connect(port).await.expect("connect CDP");
        cdp.send("Page.enable", json!({}))
            .await
            .expect("Page.enable");
        cdp.navigate("data:text/html,<html><body><h1>hello</h1></body></html>")
            .await
            .expect("navigate");
        let png = cdp.screenshot().await.expect("screenshot");
        assert!(png.starts_with(b"\x89PNG"));

        cdp.navigate(
            "data:text/html,<input id=i><script>document.getElementById('i').focus()</script>",
        )
        .await
        .expect("navigate input page");
        cdp.input(Input::Text {
            text: "hello".into(),
        })
        .await
        .expect("insert text");
        let value = cdp
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": "document.getElementById('i').value",
                    "returnByValue": true,
                }),
            )
            .await
            .expect("read input value");
        assert_eq!(
            value.pointer("/result/value").and_then(Value::as_str),
            Some("hello")
        );
    }
}
