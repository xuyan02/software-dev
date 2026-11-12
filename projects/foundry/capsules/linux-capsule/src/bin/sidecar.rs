//! Screenshot/input sidecar that runs inside the capsule container.
//!
//! It talks to the Xvfb display over the local X11 socket and exposes a tiny
//! authenticated HTTP API on port 8099.  The host library publishes that port
//! on 127.0.0.1 and never touches X11 directly.

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{xproto::*, xtest, xtest::ConnectionExt as _},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

#[derive(Clone)]
struct AppState {
    x11: Arc<Mutex<X11State>>,
    token: String,
    info: Info,
}

struct X11State {
    conn: RustConnection,
    root: Window,
    width: u16,
    height: u16,
    keymap: KeyMap,
}

#[derive(Clone, Serialize)]
struct Info {
    width: u16,
    height: u16,
    display: String,
}

struct KeyMap {
    map: HashMap<u32, (u8, bool)>,
    shift: Option<u8>,
}

impl KeyMap {
    fn build(conn: &RustConnection) -> Result<Self> {
        let setup = conn.setup();
        let min = setup.min_keycode;
        let count = setup.max_keycode - min + 1;
        let reply = conn.get_keyboard_mapping(min, count)?.reply()?;
        let per_keycode = reply.keysyms_per_keycode as usize;
        let mut map = HashMap::new();
        for (index, chunk) in reply.keysyms.chunks(per_keycode).enumerate() {
            let keycode = min + index as u8;
            for (level, &sym) in chunk.iter().take(2).enumerate() {
                if sym != 0 {
                    map.entry(sym).or_insert((keycode, level == 1));
                }
            }
        }
        let shift = map.get(&0xffe1).map(|(keycode, _)| *keycode);
        Ok(Self { map, shift })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let token = std::env::var("LINUX_CAPSULE_TOKEN").unwrap_or_default();
    if token.is_empty() {
        tracing::warn!("LINUX_CAPSULE_TOKEN is empty; the sidecar has no authentication");
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    let (conn, screen_num) = loop {
        match x11rb::connect(None) {
            Ok(value) => break value,
            Err(error) if Instant::now() < deadline => {
                tracing::debug!(%error, "waiting for X11");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => return Err(error).context("connect sidecar to X11"),
        }
    };

    let screen = &conn.setup().roots[screen_num];
    let width = screen.width_in_pixels;
    let height = screen.height_in_pixels;
    let root = screen.root;
    let depth = screen.root_depth;
    let bits_per_pixel = conn
        .setup()
        .pixmap_formats
        .iter()
        .find(|format| format.depth == depth)
        .map(|format| format.bits_per_pixel)
        .unwrap_or(32);
    if bits_per_pixel != 32 {
        anyhow::bail!("sidecar only supports 32bpp visuals (got {bits_per_pixel})");
    }
    if conn
        .extension_information(xtest::X11_EXTENSION_NAME)?
        .is_none()
    {
        anyhow::bail!("the X server does not support XTEST");
    }
    let keymap = KeyMap::build(&conn)?;

    let state = AppState {
        x11: Arc::new(Mutex::new(X11State {
            conn,
            root,
            width,
            height,
            keymap,
        })),
        token,
        info: Info {
            width,
            height,
            display: std::env::var("DISPLAY").unwrap_or_default(),
        },
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/info", get(info))
        .route("/api/screenshot", get(screenshot))
        .route("/api/input", post(input))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .with_state(state);

    let addr = std::env::var("LINUX_CAPSULE_ADDR").unwrap_or_else(|_| "0.0.0.0:8099".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind sidecar to {addr}"))?;
    tracing::info!("sidecar listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.token.is_empty() {
        return Ok(next.run(request).await);
    }
    let expected = format!("Bearer {}", state.token);
    match request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) if value == expected => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn info(State(state): State<AppState>) -> Json<Info> {
    Json(state.info)
}

async fn screenshot(State(state): State<AppState>) -> Result<Response, ApiError> {
    let x11 = Arc::clone(&state.x11);
    let (bytes, capture_ms, encode_ms) = tokio::task::spawn_blocking(move || {
        let mut guard = x11
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 state poisoned"))?;
        capture(&mut guard)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-capture-ms",
        HeaderValue::from_str(&format!("{capture_ms:.2}")).expect("digit header"),
    );
    headers.insert(
        "x-encode-ms",
        HeaderValue::from_str(&format!("{encode_ms:.2}")).expect("digit header"),
    );
    headers.insert(
        "x-frame-bytes",
        HeaderValue::from_str(&bytes.len().to_string()).expect("digit header"),
    );
    Ok((headers, bytes).into_response())
}

fn capture(state: &mut X11State) -> Result<(Vec<u8>, f64, f64)> {
    let started = Instant::now();
    let reply = state
        .conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            state.root,
            0,
            0,
            state.width,
            state.height,
            !0,
        )?
        .reply()?;
    let capture_ms = started.elapsed().as_secs_f64() * 1000.0;

    let mut data = reply.data;
    let expected = state.width as usize * state.height as usize * 4;
    if data.len() < expected {
        anyhow::bail!(
            "short frame: {} bytes for {}x{}",
            data.len(),
            state.width,
            state.height
        );
    }
    data.truncate(expected);
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }

    let started = Instant::now();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, state.width as u32, state.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    let encode_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok((out, capture_ms, encode_ms))
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InputRequest {
    Move {
        x: i16,
        y: i16,
    },
    Click {
        x: i16,
        y: i16,
        #[serde(default = "default_button")]
        button: u8,
    },
    Scroll {
        x: i16,
        y: i16,
        delta: i32,
    },
    Key {
        key: String,
    },
    Text {
        text: String,
    },
}

fn default_button() -> u8 {
    1
}

#[derive(Serialize)]
struct InputResult {
    ok: bool,
    elapsed_us: u64,
}

async fn input(
    State(state): State<AppState>,
    Json(request): Json<InputRequest>,
) -> Result<Json<InputResult>, ApiError> {
    let started = Instant::now();
    let x11 = Arc::clone(&state.x11);
    tokio::task::spawn_blocking(move || {
        let mut guard = x11
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 state poisoned"))?;
        apply_input(&mut guard, request)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(InputResult {
        ok: true,
        elapsed_us: started.elapsed().as_micros() as u64,
    }))
}

fn apply_input(state: &mut X11State, request: InputRequest) -> Result<()> {
    let conn = &state.conn;
    let root = state.root;
    let keymap = &state.keymap;

    match request {
        InputRequest::Move { x, y } => motion(conn, root, x, y)?,
        InputRequest::Click { x, y, button } => {
            let button = button.clamp(1, 7);
            motion(conn, root, x, y)?;
            conn.xtest_fake_input(BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
            conn.xtest_fake_input(BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
        }
        InputRequest::Scroll { x, y, delta } => {
            if delta != 0 {
                motion(conn, root, x, y)?;
                let button = if delta > 0 { 5 } else { 4 };
                for _ in 0..delta.unsigned_abs().min(20) {
                    conn.xtest_fake_input(BUTTON_PRESS_EVENT, button, 0, root, 0, 0, 0)?;
                    conn.xtest_fake_input(BUTTON_RELEASE_EVENT, button, 0, root, 0, 0, 0)?;
                }
            }
        }
        InputRequest::Key { key } => {
            let keysym =
                keysym_for_name(&key).ok_or_else(|| anyhow::anyhow!("unknown key name: {key}"))?;
            tap_keysym(conn, root, keymap, keysym)?;
        }
        InputRequest::Text { text } => {
            for character in text.chars() {
                let keysym = match character {
                    '\n' | '\r' => 0xff0d,
                    '\t' => 0xff09,
                    character if (' '..='~').contains(&character) => character as u32,
                    other => anyhow::bail!("only printable ASCII is supported (got {other:?})"),
                };
                tap_keysym(conn, root, keymap, keysym)?;
            }
        }
    }

    conn.flush()?;
    conn.sync()?;
    Ok(())
}

fn motion(conn: &RustConnection, root: Window, x: i16, y: i16) -> Result<()> {
    conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, x, y, 0)?;
    Ok(())
}

fn tap_keysym(conn: &RustConnection, root: Window, keymap: &KeyMap, keysym: u32) -> Result<()> {
    let (keycode, shifted) = keymap
        .map
        .get(&keysym)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("keysym {keysym:#x} is not in the X keyboard map"))?;
    let shift = if shifted {
        Some(
            keymap
                .shift
                .ok_or_else(|| anyhow::anyhow!("no shift key in the keyboard map"))?,
        )
    } else {
        None
    };

    if let Some(shift) = shift {
        conn.xtest_fake_input(KEY_PRESS_EVENT, shift, 0, root, 0, 0, 0)?;
    }
    conn.xtest_fake_input(KEY_PRESS_EVENT, keycode, 0, root, 0, 0, 0)?;
    conn.xtest_fake_input(KEY_RELEASE_EVENT, keycode, 0, root, 0, 0, 0)?;
    if let Some(shift) = shift {
        conn.xtest_fake_input(KEY_RELEASE_EVENT, shift, 0, root, 0, 0, 0)?;
    }
    Ok(())
}

fn keysym_for_name(name: &str) -> Option<u32> {
    let named = match name {
        "Return" | "Enter" => 0xff0d,
        "Tab" => 0xff09,
        "Backspace" => 0xff08,
        "Escape" | "Esc" => 0xff1b,
        "Delete" => 0xffff,
        "Insert" => 0xff63,
        "Home" => 0xff50,
        "End" => 0xff57,
        "PageUp" | "Page_Up" => 0xff55,
        "PageDown" | "Page_Down" => 0xff56,
        "Left" | "ArrowLeft" => 0xff51,
        "Up" | "ArrowUp" => 0xff52,
        "Right" | "ArrowRight" => 0xff53,
        "Down" | "ArrowDown" => 0xff54,
        "Space" => 0x20,
        "F1" => 0xffbe,
        "F2" => 0xffbf,
        "F3" => 0xffc0,
        "F4" => 0xffc1,
        "F5" => 0xffc2,
        "F6" => 0xffc3,
        "F7" => 0xffc4,
        "F8" => 0xffc5,
        "F9" => 0xffc6,
        "F10" => 0xffc7,
        "F11" => 0xffc8,
        "F12" => 0xffc9,
        _ => {
            let mut characters = name.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                return None;
            }
            character as u32
        }
    };
    Some(named)
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error.to_string())
    }
}
