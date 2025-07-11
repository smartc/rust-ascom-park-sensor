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
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tracing::info;
use tokio_util::sync::CancellationToken;

// Global to track the current serial connection
use std::sync::Mutex;
use std::sync::OnceLock;

struct SerialConnection {
    task: JoinHandle<()>,
    cancellation_token: CancellationToken,
}

static SERIAL_CONNECTION: OnceLock<Mutex<Option<SerialConnection>>> = OnceLock::new();

// Template includes
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
struct SerialCommandRequest {
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
        .route("/api/connect", post(api_connect))
        .route("/api/disconnect", post(api_disconnect))
        .route("/api/command", post(api_send_command))
        .route("/api/set_park", post(api_set_park))
        .route("/api/calibrate", post(api_calibrate))
        .route("/api/factory_reset", post(api_factory_reset))

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

#[axum::debug_handler]
async fn api_connect(
    State(state): State<SharedState>,
    Json(request): Json<ConnectRequest>,
) -> Json<ConnectResponse> {
    let baud_rate = request.baud_rate.unwrap_or(115200);
    
    info!("Connecting to {} at {} baud", request.port, baud_rate);
    
    // Stop any existing connection
    stop_serial_connection().await;
    
    // Create new cancellation token
    let cancellation_token = CancellationToken::new();
    
    // Start new serial connection task
    let device_state_clone = state.clone();
    let port = request.port.clone();
    let token_clone = cancellation_token.clone();
    
    let new_task = tokio::spawn(async move {
        if let Err(e) = crate::serial_client::run_serial_client(port, baud_rate, device_state_clone, token_clone).await {
            tracing::error!("Serial client error: {}", e);
        }
    });
    
    // Store the new connection
    let connection_mutex = SERIAL_CONNECTION.get_or_init(|| Mutex::new(None));
    if let Ok(mut current_connection) = connection_mutex.lock() {
        *current_connection = Some(SerialConnection {
            task: new_task,
            cancellation_token,
        });
    }
    
    // Update device state
    {
        let mut device_state = state.write().await;
        device_state.serial_port = Some(request.port.clone());
        device_state.clear_error();
        device_state.connected = false; // Will be set by serial client
    }
    
    Json(ConnectResponse {
        success: true,
        message: format!("Connecting to {} at {} baud", request.port, baud_rate),
    })
}

#[axum::debug_handler]
async fn api_disconnect(
    State(state): State<SharedState>,
) -> Json<ConnectResponse> {
    info!("Disconnecting from serial device");
    
    stop_serial_connection().await;
    
    // Update device state
    {
        let mut device_state = state.write().await;
        device_state.connected = false;
        device_state.serial_port = None;
        device_state.clear_error();
    }
    
    Json(ConnectResponse {
        success: true,
        message: "Disconnected from serial device".to_string(),
    })
}

async fn stop_serial_connection() {
    let connection_mutex = SERIAL_CONNECTION.get_or_init(|| Mutex::new(None));
    
    if let Ok(mut current_connection) = connection_mutex.lock() {
        if let Some(connection) = current_connection.take() {
            info!("Stopping existing serial connection");
            
            // Cancel the task
            connection.cancellation_token.cancel();
            
            // Give it a moment to cleanup
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            
            // Abort if still running
            if !connection.task.is_finished() {
                connection.task.abort();
                info!("Aborted serial task");
            }
            
            // Wait a bit more for port to be released
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }
}

// Placeholder functions for device commands (to be implemented)
async fn api_send_command(
    State(state): State<SharedState>,
    Json(request): Json<SerialCommandRequest>,
) -> Json<CommandResponse> {
    let device_state = state.read().await;
    
    if !device_state.connected {
        return Json(CommandResponse {
            success: false,
            command: request.command,
            response: None,
            message: "Device not connected".to_string(),
        });
    }
    
    info!("Manual command received: {}", request.command);
    
    Json(CommandResponse {
        success: true,
        command: request.command.clone(),
        response: Some("Command acknowledged (manual command handling not fully implemented)".to_string()),
        message: format!("Sent command: {}", request.command),
    })
}

async fn api_set_park(State(state): State<SharedState>) -> Json<ConnectResponse> {
    let device_state = state.read().await;
    
    if !device_state.connected {
        return Json(ConnectResponse {
            success: false,
            message: "Device not connected".to_string(),
        });
    }
    
    info!("Set park position command received");
    
    Json(ConnectResponse {
        success: true,
        message: "Set park position command sent".to_string(),
    })
}

async fn api_calibrate(State(state): State<SharedState>) -> Json<ConnectResponse> {
    let device_state = state.read().await;
    
    if !device_state.connected {
        return Json(ConnectResponse {
            success: false,
            message: "Device not connected".to_string(),
        });
    }
    
    info!("Calibrate sensor command received");
    
    Json(ConnectResponse {
        success: true,
        message: "Calibrate sensor command sent".to_string(),
    })
}

async fn api_factory_reset(State(state): State<SharedState>) -> Json<ConnectResponse> {
    let device_state = state.read().await;
    
    if !device_state.connected {
        return Json(ConnectResponse {
            success: false,
            message: "Device not connected".to_string(),
        });
    }
    
    info!("Factory reset command received");
    
    Json(ConnectResponse {
        success: true,
        message: "Factory reset command sent".to_string(),
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
    device.insert("DeviceName".to_string(), serde_json::Value::String("Telescope Park Sensor".to_string()));
    device.insert("DeviceType".to_string(), serde_json::Value::String("SafetyMonitor".to_string()));
    device.insert("DeviceNumber".to_string(), serde_json::Value::Number(serde_json::Number::from(0)));
    device.insert("UniqueID".to_string(), serde_json::Value::String("telescope-park-bridge-0".to_string()));
    
    Json(AlpacaResponse::success(
        vec![device],
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn management_description(Query(query): Query<AlpacaQuery>) -> Json<AlpacaResponse<HashMap<String, String>>> {
    let mut description = HashMap::new();
    description.insert("ServerName".to_string(), "Telescope Park Bridge".to_string());
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
        "nRF52840 XIAO Sense Based Position Sensor for Telescope Park Detection".to_string(),
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
    let driver_info = format!("Telescope Park Bridge v{} for {}", 
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
            "Device not connected".to_string(),
        ));
    }
    
    // Check if data is recent (within last 30 seconds)
    if !device_state.is_recent(30) {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x408,
            "Device data is stale".to_string(),
        ));
    }
    
    Json(AlpacaResponse::success(
        device_state.is_safe,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}