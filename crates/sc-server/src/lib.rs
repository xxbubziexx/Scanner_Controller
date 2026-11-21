//! `sc-server`: Axum HTTP REST & WebSocket server with embedded `static/` UI.
//!
//! Drop-in replacement for [`server.py`](../../../../server.py) (`Plan/phase_5_server_and_shadow_run.md`).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use sc_audio::{list_audio_input_devices, list_audio_output_devices};
use sc_core::{AppConfig, ConnectionMode, PriorityMode, ScanScribeFeederConfig, ScannerModel};
use sc_discovery::{detect_serial_ports, discover_lan_scanners, list_available_ports};
use sc_drivers::{HardwareTransport, Transport};
use sc_engine::{now_unix_secs, SyncManager};
use sc_feeder::CallTransmissionPayload;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

/// Embedded web UI assets from the repository `static/` directory (`Plan/phase_5_server_and_shadow_run.md` §3).
#[derive(RustEmbed)]
#[folder = "../../static"]
pub struct StaticAssets;

/// Errors returned by `sc-server`.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Config error: {0}")]
    Config(#[from] sc_core::ConfigError),
}

/// Shared application state held by the Axum router (`server.py:24-27`).
pub struct ServerState<T: Transport + Send + 'static> {
    pub sync_manager: Arc<AsyncMutex<SyncManager<T>>>,
    pub config_path: Option<PathBuf>,
    pub backed_up_once: AtomicBool,
}

impl<T: Transport + Send + 'static> ServerState<T> {
    /// Creates a new `ServerState` wrapping a `SyncManager<T>` and an optional `config.json` path.
    #[must_use]
    pub fn new(sync_manager: SyncManager<T>, config_path: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            sync_manager: Arc::new(AsyncMutex::new(sync_manager)),
            config_path,
            backed_up_once: AtomicBool::new(false),
        })
    }

    /// Enriches `config` with audio device names next to device indices and saves to `self.config_path`,
    /// creating a `.bak` backup before the first save (`Plan/phase_7_packaging_and_cutover.md` §2).
    pub fn save_config_if_configured(&self, config: &mut AppConfig) {
        enrich_config_device_names(config);
        let Some(path) = &self.config_path else {
            return;
        };
        if !self.backed_up_once.swap(true, Ordering::SeqCst) && path.exists() {
            let bak_path = path.with_extension("json.bak");
            if !bak_path.exists() {
                let _ = std::fs::copy(path, &bak_path);
            }
        }
        if let Err(err) = config
            .to_json_string()
            .and_then(|json| std::fs::write(path, json).map_err(sc_core::ConfigError::from))
        {
            tracing::error!(path = %path.display(), error = %err, "Failed to save config.json");
        }
    }
}

/// Populates `audio_device_name` and `output_device_name` next to indices when saving (`Plan/phase_7_packaging_and_cutover.md` §2).
pub fn enrich_config_device_names(config: &mut AppConfig) {
    let inputs = list_audio_input_devices();
    if let Some(idx_a) = config.scanner_a.audio_device_index {
        if let Some(dev) = inputs.iter().find(|d| d.index == idx_a as usize) {
            config.scanner_a.audio_device_name = Some(dev.name.clone());
        }
    }
    if let Some(idx_b) = config.scanner_b.audio_device_index {
        if let Some(dev) = inputs.iter().find(|d| d.index == idx_b as usize) {
            config.scanner_b.audio_device_name = Some(dev.name.clone());
        }
    }
    if let Some(idx_f) = config.feeder.audio_device_index {
        if let Some(dev) = inputs.iter().find(|d| d.index == idx_f as usize) {
            config.feeder.audio_device_name = Some(dev.name.clone());
        }
    }
    if let Some(out_idx) = config.audio_loopback.output_device_index {
        let outputs = list_audio_output_devices();
        if let Some(dev) = outputs.iter().find(|d| d.index == out_idx as usize) {
            config.audio_loopback.output_device_name = Some(dev.name.clone());
        }
    }
}

/// Builds the complete Axum [`Router`] with all REST, static, and WebSocket routes matching `server.py`.
pub fn build_router<T: Transport + Send + 'static>(state: Arc<ServerState<T>>) -> Router {
    Router::new()
        .route("/", get(handle_root))
        .route("/favicon.ico", get(handle_favicon))
        .route("/static/*path", get(handle_static_asset))
        .route("/api/health", get(handle_health_check::<T>))
        .route("/api/state", get(handle_get_state::<T>))
        .route(
            "/api/config",
            get(handle_get_config::<T>).post(handle_update_config::<T>),
        )
        .route(
            "/api/scanner/:scanner_id/model",
            post(handle_change_scanner_model::<T>),
        )
        .route("/api/skip/:scanner_id", post(handle_manual_skip::<T>))
        .route("/api/audio/devices", get(handle_audio_devices))
        .route(
            "/api/audio/output_devices",
            get(handle_audio_output_devices),
        )
        .route("/api/ports/available", get(handle_available_ports))
        .route("/api/ports/detect", get(handle_detect_ports::<T>))
        .route("/api/scanners/discover_lan", get(handle_discover_lan))
        .route("/api/models/catalog", get(handle_models_catalog))
        .route("/api/feeder/config", post(handle_update_feeder_config::<T>))
        .route(
            "/api/feeder/dispatch_test",
            post(handle_dispatch_feeder_test::<T>),
        )
        .route("/ws", get(handle_ws_upgrade::<T>))
        .with_state(state)
}

async fn handle_favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn handle_root() -> Response {
    serve_asset_bytes("index.html")
}

async fn handle_static_asset(AxumPath(path): AxumPath<String>) -> Response {
    let clean = path.trim_start_matches('/');
    serve_asset_bytes(clean)
}

fn serve_asset_bytes(rel_path: &str) -> Response {
    // Check local static/ directory first if present alongside the working directory,
    // then fall back to embedded StaticAssets (single-binary deployment).
    let local_candidate = Path::new("static").join(rel_path);
    if local_candidate.is_file() {
        if let Ok(bytes) = std::fs::read(&local_candidate) {
            let mime = mime_guess::from_path(rel_path).first_or_octet_stream();
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                bytes,
            )
                .into_response();
        }
    }

    if let Some(content) = StaticAssets::get(rel_path) {
        let mime = mime_guess::from_path(rel_path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref().to_string())],
            content.data.into_owned(),
        )
            .into_response();
    }

    if rel_path == "index.html" {
        Html("<h2>Scanner Controller Server Running</h2><p>Static index.html not found.</p>")
            .into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn handle_get_state<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
) -> Json<Value> {
    let mgr = state.sync_manager.lock().await;
    let snapshot = mgr.get_full_state();
    Json(serde_json::to_value(snapshot).unwrap_or_else(|_| json!({})))
}

async fn handle_get_config<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
) -> Json<AppConfig> {
    let mgr = state.sync_manager.lock().await;
    let full = mgr.get_full_state();
    Json(full.config)
}

async fn handle_health_check<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
) -> Json<Value> {
    let mgr = state.sync_manager.lock().await;
    let full = mgr.get_full_state();
    Json(json!({
        "status": "healthy",
        "scanner_a": {
            "connected": full.scanner_a.connected,
            "status_line": full.scanner_a.status_line
        },
        "scanner_b": {
            "connected": full.scanner_b.connected,
            "status_line": full.scanner_b.status_line
        }
    }))
}

async fn handle_update_config<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
    Json(mut new_config): Json<AppConfig>,
) -> Json<Value> {
    state.save_config_if_configured(&mut new_config);
    let mgr = state.sync_manager.lock().await;
    mgr.update_config(new_config.clone());
    Json(json!({
        "status": "success",
        "config": new_config
    }))
}

#[derive(Debug, Deserialize)]
pub struct ModelChangePayload {
    pub model: String,
}

async fn handle_change_scanner_model<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
    AxumPath(scanner_id): AxumPath<String>,
    Json(payload): Json<ModelChangePayload>,
) -> Response {
    if scanner_id != "scanner_a" && scanner_id != "scanner_b" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "detail": "Invalid scanner ID" })),
        )
            .into_response();
    }

    let Ok(new_model) =
        serde_json::from_value::<ScannerModel>(Value::String(payload.model.clone()))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "detail": "Invalid scanner model" })),
        )
            .into_response();
    };

    let mgr = state.sync_manager.lock().await;
    let mut cfg = mgr.get_full_state().config;
    if scanner_id == "scanner_a" {
        cfg.scanner_a.model = new_model;
    } else {
        cfg.scanner_b.model = new_model;
    }
    state.save_config_if_configured(&mut cfg);
    mgr.update_config(cfg);

    (
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "scanner_id": scanner_id,
            "model": payload.model
        })),
    )
        .into_response()
}

async fn handle_manual_skip<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
    AxumPath(scanner_id): AxumPath<String>,
) -> Response {
    if scanner_id != "scanner_a" && scanner_id != "scanner_b" {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "detail": "Scanner not found" })),
        )
            .into_response();
    }

    let mgr = state.sync_manager.lock().await;
    let success = mgr.manual_skip(&scanner_id);
    (
        StatusCode::OK,
        Json(json!({
            "status": if success { "success" } else { "failed" },
            "scanner_id": scanner_id
        })),
    )
        .into_response()
}

async fn handle_audio_devices() -> Json<Value> {
    let devices = tokio::task::spawn_blocking(list_audio_input_devices)
        .await
        .unwrap_or_default();
    Json(json!({
        "status": "success",
        "devices": devices
    }))
}

async fn handle_audio_output_devices() -> Json<Value> {
    let devices = tokio::task::spawn_blocking(list_audio_output_devices)
        .await
        .unwrap_or_default();
    Json(json!({
        "status": "success",
        "devices": devices
    }))
}

async fn handle_available_ports() -> Json<Value> {
    let ports = tokio::task::spawn_blocking(list_available_ports)
        .await
        .unwrap_or_default();
    Json(json!({
        "status": "success",
        "ports": ports
    }))
}

async fn handle_detect_ports<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
) -> Json<Value> {
    let cfg = {
        let mgr = state.sync_manager.lock().await;
        mgr.get_full_state().config
    };
    let mut active_ports = Vec::new();
    if cfg.scanner_a.enabled
        && cfg.scanner_a.connection_mode == ConnectionMode::Serial
        && !cfg.scanner_a.port.eq_ignore_ascii_case("NONE")
    {
        active_ports.push(cfg.scanner_a.port.clone());
    }
    if cfg.scanner_b.enabled
        && cfg.scanner_b.connection_mode == ConnectionMode::Serial
        && !cfg.scanner_b.port.eq_ignore_ascii_case("NONE")
    {
        active_ports.push(cfg.scanner_b.port.clone());
    }

    let detected = tokio::task::spawn_blocking(move || detect_serial_ports(&active_ports))
        .await
        .unwrap_or_default();
    Json(json!({
        "status": "success",
        "ports": detected
    }))
}

async fn handle_discover_lan() -> Json<Value> {
    let discovered =
        tokio::task::spawn_blocking(|| discover_lan_scanners(Duration::from_millis(1200)))
            .await
            .unwrap_or_default();
    Json(json!({
        "status": "success",
        "scanners": discovered
    }))
}

#[derive(Debug, Serialize)]
struct ModelCatalogEntry {
    model: &'static str,
    allowed_baud_rates: &'static [u32],
}

async fn handle_models_catalog() -> Json<Value> {
    let models: Vec<ModelCatalogEntry> = ScannerModel::ALL
        .iter()
        .map(|m| ModelCatalogEntry {
            model: m.as_str(),
            allowed_baud_rates: m.allowed_baud_rates(),
        })
        .collect();
    Json(json!({
        "status": "success",
        "models": models
    }))
}

async fn handle_update_feeder_config<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
    Json(new_feeder_cfg): Json<ScanScribeFeederConfig>,
) -> Json<Value> {
    let mgr = state.sync_manager.lock().await;
    let mut cfg = mgr.get_full_state().config;
    cfg.feeder = new_feeder_cfg;
    state.save_config_if_configured(&mut cfg);
    mgr.update_config(cfg.clone());
    Json(json!({
        "status": "success",
        "feeder_config": cfg.feeder
    }))
}

#[derive(Debug, Default, Deserialize)]
pub struct FeederTestRequest {
    pub scanner_id: Option<String>,
}

async fn handle_dispatch_feeder_test<T: Transport + Send + 'static>(
    State(state): State<Arc<ServerState<T>>>,
    Query(query): Query<FeederTestRequest>,
    body: Option<Json<FeederTestRequest>>,
) -> Json<Value> {
    let raw_id = body
        .and_then(|b| b.0.scanner_id)
        .or(query.scanner_id)
        .unwrap_or_else(|| "scanner_a".to_string());
    let target_id = if raw_id == "scanner_b" {
        "scanner_b".to_string()
    } else {
        "scanner_a".to_string()
    };

    let (feeder_arc, sc_cfg) = {
        let mgr = state.sync_manager.lock().await;
        let cfg = mgr.get_full_state().config;
        let sc = if target_id == "scanner_a" {
            cfg.scanner_a
        } else {
            cfg.scanner_b
        };
        (Arc::clone(&mgr.feeder), sc)
    };

    let target_clone = target_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let payload = CallTransmissionPayload {
            scanner_id: target_clone,
            scanner_model: sc_cfg.model.as_str().to_string(),
            system_name: "Metropolitan P25 Trunk".to_string(),
            department_name: "Fire & Rescue".to_string(),
            channel_name: "Dispatch North".to_string(),
            tgid: Some("10401".to_string()),
            frequency: Some(851.2500),
            modulation: "NFM".to_string(),
            rssi: 5,
            timestamp: now_unix_secs(),
            duration_seconds: 5.4,
            audio_pcm: None,
            filename_template: sc_cfg.filename_template,
            tit2_template: sc_cfg.tit2_template,
        };
        if let Ok(fdr) = feeder_arc.lock() {
            serde_json::to_value(fdr.dispatch_call(payload)).unwrap_or_else(|_| json!({}))
        } else {
            json!({ "status": "error", "error": "Feeder mutex poisoned" })
        }
    })
    .await
    .unwrap_or_else(|e| json!({ "status": "error", "error": e.to_string() }));

    Json(json!({
        "status": "success",
        "scanner_id": target_id,
        "dispatch_result": result
    }))
}

async fn handle_ws_upgrade<T: Transport + Send + 'static>(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState<T>>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection<T: Transport + Send + 'static>(
    socket: WebSocket,
    state: Arc<ServerState<T>>,
) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial state snapshot immediately on connect (server.py:259)
    let (initial_snapshot, mut rx_watch) = {
        let mgr = state.sync_manager.lock().await;
        (mgr.get_full_state(), mgr.subscribe_state())
    };

    if let Ok(initial_txt) = serde_json::to_string(&initial_snapshot) {
        if sender.send(Message::Text(initial_txt)).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            changed = rx_watch.changed() => {
                if changed.is_err() {
                    break;
                }
                let snapshot = rx_watch.borrow_and_update().clone();
                if let Ok(txt) = serde_json::to_string(&snapshot) {
                    if sender.send(Message::Text(txt)).await.is_err() {
                        break;
                    }
                }
            }
            msg_opt = receiver.next() => {
                let Some(Ok(msg)) = msg_opt else {
                    break;
                };
                if let Message::Text(text) = msg {
                    if let Ok(data) = serde_json::from_str::<Value>(&text) {
                        process_ws_command(&state, &data).await;
                    }
                } else if matches!(msg, Message::Close(_)) {
                    break;
                }
            }
        }
    }
}

async fn process_ws_command<T: Transport + Send + 'static>(
    state: &Arc<ServerState<T>>,
    data: &Value,
) {
    let Some(cmd_type) = data.get("type").and_then(Value::as_str) else {
        return;
    };

    match cmd_type {
        "SKIP" => {
            if let Some(scanner_id) = data.get("scanner_id").and_then(Value::as_str) {
                let mgr = state.sync_manager.lock().await;
                let _ = mgr.manual_skip(scanner_id);
            }
        }
        "TOGGLE_SYNC" => {
            let enabled = data.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let mgr = state.sync_manager.lock().await;
            let mut cfg = mgr.get_full_state().config;
            cfg.sync.enable_sync = enabled;
            state.save_config_if_configured(&mut cfg);
            mgr.update_config(cfg);
        }
        "TOGGLE_RECORD" => {
            let enabled = data.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let mgr = state.sync_manager.lock().await;
            let mut cfg = mgr.get_full_state().config;
            cfg.feeder.enable_feeder = enabled;
            state.save_config_if_configured(&mut cfg);
            mgr.set_recording_enabled(enabled);
        }
        "SET_PRIORITY" => {
            if let Some(p_str) = data.get("priority_mode").and_then(Value::as_str) {
                if let Ok(p_mode) =
                    serde_json::from_value::<PriorityMode>(Value::String(p_str.to_string()))
                {
                    let mgr = state.sync_manager.lock().await;
                    let mut cfg = mgr.get_full_state().config;
                    cfg.sync.priority_mode = p_mode;
                    state.save_config_if_configured(&mut cfg);
                    mgr.update_config(cfg);
                }
            }
        }
        "CHANGE_MODEL" => {
            let scanner_id = data.get("scanner_id").and_then(Value::as_str);
            let model_str = data.get("model").and_then(Value::as_str);
            if let (Some(id), Some(m_str)) = (scanner_id, model_str) {
                if (id == "scanner_a" || id == "scanner_b")
                    && serde_json::from_value::<ScannerModel>(Value::String(m_str.to_string()))
                        .is_ok()
                {
                    if let Ok(new_model) =
                        serde_json::from_value::<ScannerModel>(Value::String(m_str.to_string()))
                    {
                        let mgr = state.sync_manager.lock().await;
                        let mut cfg = mgr.get_full_state().config;
                        if id == "scanner_a" {
                            cfg.scanner_a.model = new_model;
                        } else {
                            cfg.scanner_b.model = new_model;
                        }
                        state.save_config_if_configured(&mut cfg);
                        mgr.update_config(cfg);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Starts the live hardware `SyncManager` and serves the Axum HTTP + WebSocket dashboard on `addr`,
/// shutting down gracefully when `shutdown_token` is cancelled (`Plan/phase_5_server_and_shadow_run.md` §3).
///
/// # Errors
///
/// Returns [`ServerError::Io`] if binding `addr` fails.
pub async fn run_hardware_server(
    config: AppConfig,
    config_path: Option<PathBuf>,
    addr: SocketAddr,
    shutdown_token: CancellationToken,
) -> Result<(), ServerError> {
    let mut sync_manager: SyncManager<HardwareTransport> = SyncManager::new_hardware(config);
    sync_manager.start();

    let state = ServerState::new(sync_manager, config_path);
    let app = build_router(Arc::clone(&state));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        address = %addr,
        "Uniden Scanner Controller Web Dashboard listening on http://{}",
        addr
    );

    let token_clone = shutdown_token.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            token_clone.cancelled().await;
        })
        .await?;

    tracing::info!("Shutting down Scanner Sync Engine...");
    state.sync_manager.lock().await.stop().await;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use sc_drivers::ReplayTransport;
    use tower::ServiceExt;

    fn api_fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("api")
    }

    fn make_test_router() -> Router {
        let cfg = AppConfig::default();
        let mgr: SyncManager<ReplayTransport> = SyncManager::with_drivers(cfg, None, None);
        let state = ServerState::new(mgr, None);
        build_router(state)
    }

    #[tokio::test]
    async fn test_api_contract_fixtures_parity() {
        let app = make_test_router();

        // 1. Verify GET /api/models/catalog matches rust/fixtures/api/models_catalog.json 100%
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/models/catalog")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let actual_catalog: Value = serde_json::from_slice(&body_bytes).unwrap();
        let expected_catalog: Value = serde_json::from_str(
            &std::fs::read_to_string(api_fixtures_dir().join("models_catalog.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(actual_catalog, expected_catalog);

        // 2. Verify GET /api/state matches rust/fixtures/api/initial_state.json (ignoring last_update timestamps)
        let resp_state = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp_state.status(), StatusCode::OK);
        let state_bytes = resp_state.into_body().collect().await.unwrap().to_bytes();
        let mut actual_state: Value = serde_json::from_slice(&state_bytes).unwrap();
        let mut expected_state: Value = serde_json::from_str(
            &std::fs::read_to_string(api_fixtures_dir().join("initial_state.json")).unwrap(),
        )
        .unwrap();

        actual_state["scanner_a"]["last_update"] = json!(0.0);
        actual_state["scanner_b"]["last_update"] = json!(0.0);
        expected_state["scanner_a"]["last_update"] = json!(0.0);
        expected_state["scanner_b"]["last_update"] = json!(0.0);

        assert_eq!(actual_state, expected_state);
    }

    #[tokio::test]
    async fn test_static_ui_and_mutation_endpoints() {
        let app = make_test_router();

        // 1. GET / serves embedded index.html
        let root_resp = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(root_resp.status(), StatusCode::OK);
        let root_html = String::from_utf8(
            root_resp
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(root_html.contains("Uniden Dynamic Scanner Sync"));

        // 2. GET /static/app.js and /static/style.css
        let js_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/static/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(js_resp.status(), StatusCode::OK);

        let css_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/static/style.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(css_resp.status(), StatusCode::OK);

        // 3. GET /favicon.ico -> 204 No Content
        let fav_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/favicon.ico")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(fav_resp.status(), StatusCode::NO_CONTENT);

        // 4. GET /api/config -> 200 OK
        let cfg_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cfg_resp.status(), StatusCode::OK);
        let cfg_json: Value =
            serde_json::from_slice(&cfg_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(cfg_json["scanner_a"]["id"], "scanner_a");

        // 5. GET /api/health -> 200 OK
        let health_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health_resp.status(), StatusCode::OK);
        let health_json: Value =
            serde_json::from_slice(&health_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(health_json["status"], "healthy");

        // 6. POST /api/scanner/scanner_a/model
        let model_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/scanner/scanner_a/model")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"SDS100"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(model_resp.status(), StatusCode::OK);
        let model_json: Value =
            serde_json::from_slice(&model_resp.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(model_json["status"], "success");
        assert_eq!(model_json["model"], "SDS100");
    }
}
