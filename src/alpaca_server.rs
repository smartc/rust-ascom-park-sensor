use crate::device_state::DeviceState;
use axum::{
    extract::{Path, Query, State},
    response::Html,
    routing::{get, post},
    Router, Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tracing::info;

use tokio_util::sync::CancellationToken;

// Global to track the current serial connection task and cancellation
use std::sync::Mutex;
use std::sync::OnceLock;

static SERIAL_TASK: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();
static SERIAL_CANCELLATION: OnceLock<Mutex<Option<CancellationToken>>> = OnceLock::new();

// External template files
const INDEX_HTML: &str = include_str!("../templates/index.html");
const STYLE_CSS: &str = include_str!("../templates/style.css");
const SCRIPT_JS: &str = include_str!("../templates/script.js");

// ASCOM Alpaca response structure
#[derive(Serialize)]
struct AlpacaResponse<T> {
    #[serde(rename = "Value")]
    value: T,
    #[serde(rename = "ClientTransactionID")]
    client_transaction_id: u32,
    #[serde(rename = "ServerTransactionID")]
    server_transaction_id: u32,
    #[serde(rename = "ErrorNumber")]
    error_number: u32,
    #[serde(rename = "ErrorMessage")]
    error_message: String,
}

impl<T> AlpacaResponse<T> {
    fn success(value: T, client_transaction_id: u32, server_transaction_id: u32) -> Self {
        Self {
            value,
            client_transaction_id,
            server_transaction_id,
            error_number: 0,
            error_message: String::new(),
        }
    }
    
    fn error(value: T, client_transaction_id: u32, server_transaction_id: u32, error_number: u32, error_message: String) -> Self {
        Self {
            value,
            client_transaction_id,
            server_transaction_id,
            error_number,
            error_message,
        }
    }
}

#[derive(Deserialize)]
struct AlpacaQuery {
    #[serde(rename = "ClientTransactionID")]
    client_transaction_id: Option<u32>,
}

#[derive(Deserialize)]
struct ConnectRequest {
    port: String,
    baud_rate: Option<u32>,
}

#[derive(Deserialize)]
struct CommandRequest {
    command: String,
}

#[derive(Serialize)]
struct PortListResponse {
    ports: Vec<crate::port_discovery::PortInfo>,
}

#[derive(Serialize)]
struct ConnectResponse {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct CommandResponse {
    success: bool,
    command: String,
    response: Option<String>,
    message: String,
}

type SharedState = Arc<RwLock<DeviceState>>;

pub async fn create_alpaca_server(
    bind_address: String,
    port: u16,
    device_state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = create_router(device_state);
    
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_address, port)).await?;
    
    info!("ASCOM Alpaca server listening on {}:{}", bind_address, port);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

fn create_router(device_state: SharedState) -> Router {
    Router::new()
        // Web interface routes
        .route("/", get(web_interface))
        .route("/api/status", get(api_status))
        .route("/api/ports", get(api_ports))
        .route("/api/connect", post({
            |State(state): State<SharedState>, Json(request): Json<ConnectRequest>| async move {
                api_connect(state, request).await
            }
        }))
        .route("/api/disconnect", post({
            |State(state): State<SharedState>| async move {
                api_disconnect(state).await
            }
        }))
        .route("/api/command", post(api_send_command))
        
        // Device control routes
        .route("/api/device/calibrate", post(api_calibrate))
        .route("/api/device/set_park", post(api_set_park))
        .route("/api/device/factory_reset", post(api_factory_reset))

        // ASCOM Alpaca Management API
        .route("/management/apiversions", get(management_api_versions))
        .route("/management/v1/configureddevices", get(management_configured_devices))
        .route("/management/v1/description", get(management_description))
        
        // ASCOM Alpaca Safety Monitor API
        .route("/api/v1/safetymonitor/:device_number/connected", 
               get(get_connected))
        .route("/api/v1/safetymonitor/:device_number/description", get(get_description))
        .route("/api/v1/safetymonitor/:device_number/driverinfo", get(get_driver_info))
        .route("/api/v1/safetymonitor/:device_number/driverversion", get(get_driver_version))
        .route("/api/v1/safetymonitor/:device_number/interfaceversion", get(get_interface_version))
        .route("/api/v1/safetymonitor/:device_number/name", get(get_name))
        .route("/api/v1/safetymonitor/:device_number/supportedactions", get(get_supported_actions))
        .route("/api/v1/safetymonitor/:device_number/issafe", get(get_is_safe))
        
        .layer(CorsLayer::permissive())
        .with_state(device_state)
}

static mut SERVER_TRANSACTION_ID: u32 = 0;

fn next_server_transaction_id() -> u32 {
    unsafe {
        SERVER_TRANSACTION_ID += 1;
        SERVER_TRANSACTION_ID
    }
}

// Web interface handler
async fn web_interface() -> Html<String> {
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS);
    
    Html(html)
}

async fn api_status(State(state): State<SharedState>) -> Json<DeviceState> {
    let device_state = state.read().await;
    Json(device_state.clone())
}

async fn api_ports() -> Json<PortListResponse> {
    match crate::port_discovery::discover_ports() {
        Ok(ports) => Json(PortListResponse { ports }),
        Err(_) => Json(PortListResponse { ports: vec![] }),
    }
}

async fn api_connect(state: SharedState, request: ConnectRequest) -> Json<ConnectResponse> {
    let baud_rate = request.baud_rate.unwrap_or(115200);
    
    // Cancel and abort any existing connection
    let cancel_mutex = SERIAL_CANCELLATION.get_or_init(|| Mutex::new(None));
    if let Ok(mut current_cancel) = cancel_mutex.lock() {
        if let Some(cancel_token) = current_cancel.take() {
            cancel_token.cancel();
        }
    }
    
    let task_mutex = SERIAL_TASK.get_or_init(|| Mutex::new(None));
    let task_to_abort = {
        if let Ok(mut current_task) = task_mutex.lock() {
            current_task.take()
        } else {
            None
        }
    };
    
    if let Some(task) = task_to_abort {
        info!("Aborting existing serial task");
        task.abort();
        match tokio::time::timeout(Duration::from_millis(1000), task).await {
            Ok(_) => info!("Previous serial task stopped cleanly"),
            Err(_) => info!("Previous serial task abort timed out"),
        }
    }
    
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Create new cancellation token
    let cancel_token = CancellationToken::new();
    if let Ok(mut current_cancel) = cancel_mutex.lock() {
        *current_cancel = Some(cancel_token.clone());
    }
    
    // Start new serial connection task with cancellation
    let device_state_clone = state.clone();
    let port = request.port.clone();
    
    let new_task = tokio::spawn(async move {
        if let Err(e) = crate::serial_client::run_serial_client_with_cancellation(port, baud_rate, device_state_clone, cancel_token).await {
            tracing::error!("Serial client error: {}", e);
        }
    });
    
    if let Ok(mut current_task) = task_mutex.lock() {
        *current_task = Some(new_task);
    }
    
    {
        let mut device_state = state.write().await;
        device_state.serial_port = Some(request.port.clone());
        device_state.clear_error();
    }
    
    Json(ConnectResponse {
        success: true,
        message: format!("Connecting to nRF52840 device on {} at {} baud", request.port, baud_rate),
    })
}

async fn api_disconnect(state: SharedState) -> Json<ConnectResponse> {
    info!("Disconnecting from nRF52840 device");
    
    // Cancel the serial operation first
    let cancel_mutex = SERIAL_CANCELLATION.get_or_init(|| Mutex::new(None));
    if let Ok(mut current_cancel) = cancel_mutex.lock() {
        if let Some(cancel_token) = current_cancel.take() {
            info!("Cancelling serial operations");
            cancel_token.cancel();
        }
    }
    
    // Then abort the task
    let task_mutex = SERIAL_TASK.get_or_init(|| Mutex::new(None));
    let task_to_abort = {
        if let Ok(mut current_task) = task_mutex.lock() {
            current_task.take()
        } else {
            None
        }
    };
    
    if let Some(task) = task_to_abort {
        info!("Aborting serial task");
        task.abort();
        match tokio::time::timeout(Duration::from_millis(2000), task).await {
            Ok(_) => info!("Serial task stopped cleanly"),
            Err(_) => info!("Serial task abort timed out"),
        }
    }
    
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    {
        let mut device_state = state.write().await;
        device_state.connected = false;
        device_state.serial_port = None;
        device_state.clear_error();
    }
    
    info!("Serial port released and device disconnected");
    
    Json(ConnectResponse {
        success: true,
        message: "Disconnected from nRF52840 device and released serial port".to_string(),
    })
}

async fn api_send_command(State(_state): State<SharedState>, Json(request): Json<CommandRequest>) -> Json<CommandResponse> {
    // TODO: Implement command sending
    // This will require refactoring the serial client to expose a command channel
    Json(CommandResponse {
        success: false,
        command: request.command,
        response: None,
        message: "Command sending not yet implemented - will be added in next update".to_string(),
    })
}

async fn api_calibrate(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    // TODO: Send calibration command (06)
    Json(ConnectResponse {
        success: false,
        message: "Calibration command not yet implemented - will be added in next update".to_string(),
    })
}

async fn api_set_park(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    // TODO: Send set park command (04 or 0D)
    Json(ConnectResponse {
        success: false,
        message: "Set park command not yet implemented - will be added in next update".to_string(),
    })
}

async fn api_factory_reset(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    // TODO: Send factory reset command (0E)
    Json(ConnectResponse {
        success: false,
        message: "Factory reset command not yet implemented - will be added in next update".to_string(),
    })
}

// ASCOM Management API handlers
async fn management_api_versions(Query(query): Query<AlpacaQuery>) -> Json<AlpacaResponse<Vec<u32>>> {
    Json(AlpacaResponse::success(
        vec![1],
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn management_configured_devices(Query(query): Query<AlpacaQuery>) -> Json<AlpacaResponse<Vec<HashMap<String, serde_json::Value>>>> {
    let mut device = HashMap::new();
    device.insert("DeviceName".to_string(), serde_json::Value::String("nRF52840 Telescope Park Sensor".to_string()));
    device.insert("DeviceType".to_string(), serde_json::Value::String("SafetyMonitor".to_string()));
    device.insert("DeviceNumber".to_string(), serde_json::Value::Number(serde_json::Number::from(0)));
    device.insert("UniqueID".to_string(), serde_json::Value::String("nrf52840-park-sensor-0".to_string()));
    
    Json(AlpacaResponse::success(
        vec![device],
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn management_description(Query(query): Query<AlpacaQuery>) -> Json<AlpacaResponse<HashMap<String, String>>> {
    let mut description = HashMap::new();
    description.insert("ServerName".to_string(), "nRF52840 Telescope Park Bridge".to_string());
    description.insert("Manufacturer".to_string(), "Corey Smart".to_string());
    description.insert("ManufacturerVersion".to_string(), env!("CARGO_PKG_VERSION").to_string());
    description.insert("Location".to_string(), "Local".to_string());
    
    Json(AlpacaResponse::success(
        description,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

// ASCOM Safety Monitor API handlers
async fn get_connected(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<bool>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    let device_state = state.read().await;
    Json(AlpacaResponse::success(
        device_state.connected,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_description(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<SharedState>,
) -> Json<AlpacaResponse<String>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    Json(AlpacaResponse::success(
        "nRF52840 XIAO Sense Based Telescope Park Position Sensor with Built-in IMU".to_string(),
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_driver_info(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<String>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    let device_state = state.read().await;
    let driver_info = format!("nRF52840 Telescope Park Bridge v{} for {}", 
        env!("CARGO_PKG_VERSION"), 
        device_state.device_name
    );
    
    Json(AlpacaResponse::success(
        driver_info,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_driver_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<String>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    Json(AlpacaResponse::success(
        env!("CARGO_PKG_VERSION").to_string(),
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_interface_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<u32>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            0,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    Json(AlpacaResponse::success(
        1,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_name(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<String>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    let device_state = state.read().await;
    Json(AlpacaResponse::success(
        device_state.device_name.clone(),
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_supported_actions(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<Vec<String>>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            vec![],
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    Json(AlpacaResponse::success(
        vec![], // No custom actions supported
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_is_safe(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<bool>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    let device_state = state.read().await;
    
    // If not connected, it's not safe
    if !device_state.connected {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x407,
            "nRF52840 device not connected".to_string(),
        ));
    }
    
    // Check if data is recent (within last 30 seconds)
    if !device_state.is_recent(30) {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x408,
            "nRF52840 device data is stale".to_string(),
        ));
    }
    
    Json(AlpacaResponse::success(
        device_state.is_safe,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}