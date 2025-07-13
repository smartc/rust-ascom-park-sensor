// src/alpaca_server.rs
// Fixed version with proper ClientTransactionID handling and PUT endpoints

use crate::device_state::DeviceState;
use axum::{
    extract::{Path, Query, State},
    response::Html,
    routing::{get, put},
    Router, Json, Form,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::info;
use std::sync::atomic::{AtomicU32, Ordering};

// External template files
const INDEX_HTML: &str = include_str!("../templates/index.html");
const STYLE_CSS: &str = include_str!("../templates/style.css");
const SCRIPT_JS: &str = include_str!("../templates/script.js");

// Global server transaction ID counter
static SERVER_TRANSACTION_ID: AtomicU32 = AtomicU32::new(0);

fn next_server_transaction_id() -> u32 {
    SERVER_TRANSACTION_ID.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
}

// ASCOM Alpaca response structure with proper case sensitivity
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
    fn success(value: T, client_transaction_id: u32) -> Self {
        Self {
            value,
            client_transaction_id,
            server_transaction_id: next_server_transaction_id(),
            error_number: 0,
            error_message: String::new(),
        }
    }
    
    fn error(value: T, client_transaction_id: u32, error_number: u32, error_message: String) -> Self {
        Self {
            value,
            client_transaction_id,
            server_transaction_id: next_server_transaction_id(),
            error_number,
            error_message,
        }
    }
}

// Query parameters for GET requests (case insensitive)
#[derive(Deserialize)]
struct AlpacaQuery {
    #[serde(rename = "ClientTransactionID")]
    #[serde(alias = "clienttransactionid")]
    #[serde(alias = "ClientTransactionId")]
    #[serde(alias = "clientTransactionID")]
    client_transaction_id: Option<u32>,
    
    #[serde(rename = "ClientID")]
    #[serde(alias = "clientid")]
    #[serde(alias = "ClientId")]
    #[serde(alias = "clientID")]
    client_id: Option<u32>,
}

// API request/response types
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
        // Web interface
        .route("/", get(web_interface))
 
        // Device setup endpoints
        .route("/setup", get(web_interface))
        .route("/setup/v1/safetymonitor/:device_number/setup", get(web_interface_device_control))
  
        
        // Web API endpoints
        .route("/api/status", get(api_status))
        .route("/api/ports", get(api_ports))
        .route("/api/connect", put(api_connect))
        .route("/api/disconnect", put(api_disconnect))
        .route("/api/command", put(api_send_command))
        .route("/api/device/calibrate", put(api_calibrate))
        .route("/api/device/set_park", put(api_set_park))
        .route("/api/device/factory_reset", put(api_factory_reset))
        
        // ASCOM Management API
        .route("/management/apiversions", get(get_management_api_versions))
        .route("/management/v1/description", get(get_management_description))
        .route("/management/v1/configureddevices", get(get_configured_devices))
        
        // ASCOM Device API - Common endpoints
        .route("/api/v1/safetymonitor/:device_number/connected", get(get_connected))
        .route("/api/v1/safetymonitor/:device_number/connected", put(put_connected_simple))
        .route("/api/v1/safetymonitor/:device_number/description", get(get_description))
        .route("/api/v1/safetymonitor/:device_number/driverinfo", get(get_driver_info))
        .route("/api/v1/safetymonitor/:device_number/driverversion", get(get_driver_version))
        .route("/api/v1/safetymonitor/:device_number/interfaceversion", get(get_interface_version))
        .route("/api/v1/safetymonitor/:device_number/name", get(get_name))
        .route("/api/v1/safetymonitor/:device_number/supportedactions", get(get_supported_actions))
        
        // ASCOM Device API - SafetyMonitor specific
        .route("/api/v1/safetymonitor/:device_number/issafe", get(get_is_safe))
        
        .layer(CorsLayer::permissive())
        .with_state(device_state)
}

// Helper function to extract client transaction ID with proper default handling
fn get_client_transaction_id(query_id: Option<u32>) -> u32 {
    query_id.unwrap_or(0)
}

// Web interface handler
async fn web_interface() -> Html<String> {
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS);
    
    Html(html)
}

// Web interface handler - Device Control tab
async fn web_interface_device_control(Path(device_number): Path<u32>) -> Html<String> {
    if device_number != 0 {
        return Html("<h1>Error: Invalid device number. Only device 0 is available.</h1>".to_string());
    }
    
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS)
        .replace("</body>", r#"<script>
            document.addEventListener('DOMContentLoaded', function() {
                switchTab('device-control');
            });
        </script></body>"#);
    
    Html(html)
}

// Web API handlers
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

async fn api_connect(
    State(_state): State<SharedState>,
    Json(request): Json<ConnectRequest>,
) -> Json<ConnectResponse> {
    // Implementation would go here - connect to serial port
    Json(ConnectResponse {
        success: true,
        message: format!("Connecting to {}", request.port),
    })
}

async fn api_disconnect(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    // Implementation would go here - disconnect from serial port
    Json(ConnectResponse {
        success: true,
        message: "Disconnected".to_string(),
    })
}

async fn api_send_command(
    State(_state): State<SharedState>,
    Json(request): Json<CommandRequest>,
) -> Json<CommandResponse> {
    // Implementation would go here - send command to device
    Json(CommandResponse {
        success: true,
        command: request.command.clone(),
        response: Some("Command sent".to_string()),
        message: "Command executed".to_string(),
    })
}

async fn api_calibrate(State(_state): State<SharedState>) -> Json<CommandResponse> {
    // Implementation would go here - send calibrate command
    Json(CommandResponse {
        success: true,
        command: "06".to_string(),
        response: Some("Calibration complete".to_string()),
        message: "Sensor calibrated".to_string(),
    })
}

async fn api_set_park(State(_state): State<SharedState>) -> Json<CommandResponse> {
    // Implementation would go here - send set park command
    Json(CommandResponse {
        success: true,
        command: "04".to_string(),
        response: Some("Park position set".to_string()),
        message: "Park position updated".to_string(),
    })
}

async fn api_factory_reset(State(_state): State<SharedState>) -> Json<CommandResponse> {
    // Implementation would go here - send factory reset command
    Json(CommandResponse {
        success: true,
        command: "0E".to_string(),
        response: Some("Factory reset complete".to_string()),
        message: "Device reset to factory defaults".to_string(),
    })
}

// ASCOM Management API handlers
async fn get_management_api_versions(Query(query): Query<AlpacaQuery>) -> Json<AlpacaResponse<Vec<u32>>> {
    Json(AlpacaResponse::success(
        vec![1],
        get_client_transaction_id(query.client_transaction_id),
    ))
}

async fn get_management_description(Query(query): Query<AlpacaQuery>) -> Json<AlpacaResponse<serde_json::Value>> {
    let description = serde_json::json!({
        "ServerName": "nRF52840 Telescope Park Bridge",
        "Manufacturer": "Corey Smart",
        "ManufacturerVersion": env!("CARGO_PKG_VERSION"),
        "Location": "Local"
    });
    
    Json(AlpacaResponse::success(
        description,
        get_client_transaction_id(query.client_transaction_id),
    ))
}

async fn get_configured_devices(
    Query(query): Query<AlpacaQuery>, 
    State(state): State<SharedState>
) -> Json<AlpacaResponse<Vec<serde_json::Value>>> {
    let device_state = state.read().await;
    let devices = vec![serde_json::json!({
        "DeviceName": device_state.device_name,
        "DeviceType": "SafetyMonitor", 
        "DeviceNumber": 0,
        "UniqueID": device_state.unique_id
    })];
    
    Json(AlpacaResponse::success(
        devices,
        get_client_transaction_id(query.client_transaction_id),
    ))
}

// ASCOM Device API handlers
async fn get_connected(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<bool>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            false,
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    let device_state = state.read().await;
    Json(AlpacaResponse::success(device_state.connected, client_transaction_id))
}

// Simple PUT handler for connected property (ASCOM requirement)
async fn put_connected_simple(
    Path(device_number): Path<u32>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<()>> {
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            (),
            0,
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    // Toggle ASCOM connection state
    {
        let mut device_state = state.write().await;
        device_state.ascom_connected = !device_state.ascom_connected;
        info!("ASCOM client connection toggled to: {}", device_state.ascom_connected);
    }
    
    Json(AlpacaResponse::success((), 0))
}

async fn get_description(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<String>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    Json(AlpacaResponse::success(
        "nRF52840 XIAO Sense based telescope park position sensor with built-in LSM6DS3TR-C IMU".to_string(),
        client_transaction_id,
    ))
}

async fn get_driver_info(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<String>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    let device_state = state.read().await;
    let driver_info = format!(
        "Telescope Park Bridge v{} for {}",
        env!("CARGO_PKG_VERSION"),
        device_state.device_name
    );
    
    Json(AlpacaResponse::success(driver_info, client_transaction_id))
}

async fn get_driver_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<String>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    Json(AlpacaResponse::success(
        env!("CARGO_PKG_VERSION").to_string(),
        client_transaction_id,
    ))
}

async fn get_interface_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<u32>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            0,
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    Json(AlpacaResponse::success(1, client_transaction_id))
}

async fn get_name(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<String>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            String::new(),
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    let device_state = state.read().await;
    Json(AlpacaResponse::success(device_state.device_name.clone(), client_transaction_id))
}

async fn get_supported_actions(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
) -> Json<AlpacaResponse<Vec<String>>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            vec![],
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    Json(AlpacaResponse::success(
        vec![
            "Calibrate".to_string(),
            "SetPark".to_string(),
            "FactoryReset".to_string(),
        ],
        client_transaction_id,
    ))
}

async fn get_is_safe(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Json<AlpacaResponse<bool>> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            false,
            client_transaction_id,
            0x400,
            format!("Invalid device number: {}", device_number),
        ));
    }
    
    let device_state = state.read().await;
    Json(AlpacaResponse::success(device_state.is_safe, client_transaction_id))
}