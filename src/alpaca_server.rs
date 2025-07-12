use crate::device_state::DeviceState;
use crate::connection_manager::ConnectionManager;
use axum::{
    extract::{Path, Query, State},
    response::Html,
    routing::{get, post, put},
    Router, Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::{info, error, debug};

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
type SharedConnectionManager = Arc<ConnectionManager>;

pub async fn create_alpaca_server(
    bind_address: String,
    port: u16,
    device_state: SharedState,
    connection_manager: SharedConnectionManager,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = create_router(device_state, connection_manager);
    
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_address, port)).await?;
    
    info!("ASCOM Alpaca server listening on {}:{}", bind_address, port);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

fn create_router(device_state: SharedState, connection_manager: SharedConnectionManager) -> Router {
    Router::new()
        // Web interface routes
        .route("/", get(web_interface))
        .route("/api/status", get(api_status))
        .route("/api/ports", get(api_ports))
        .route("/api/connect", post(api_connect))
        .route("/api/disconnect", post(api_disconnect))
        .route("/api/command", post(api_send_command))
        
        // Device control routes
        .route("/api/device/calibrate", post(api_calibrate))
        .route("/api/device/set_park", post(api_set_park))
        .route("/api/device/factory_reset", post(api_factory_reset))

        // ASCOM Alpaca Management API
        .route("/management/apiversions", get(management_api_versions))
        .route("/management/v1/configureddevices", get(management_configured_devices))
        .route("/management/v1/description", get(management_description))
        
        // ASCOM Alpaca Safety Monitor API - Fixed for v0.4.0
        .route("/api/v1/safetymonitor/:device_number/connected", 
               get(get_connected))
        .route("/api/v1/safetymonitor/:device_number/connected", 
               put(put_connected_simple))
        .route("/api/v1/safetymonitor/:device_number/description", get(get_description))
        .route("/api/v1/safetymonitor/:device_number/driverinfo", get(get_driver_info))
        .route("/api/v1/safetymonitor/:device_number/driverversion", get(get_driver_version))
        .route("/api/v1/safetymonitor/:device_number/interfaceversion", get(get_interface_version))
        .route("/api/v1/safetymonitor/:device_number/name", get(get_name))
        .route("/api/v1/safetymonitor/:device_number/supportedactions", get(get_supported_actions))
        .route("/api/v1/safetymonitor/:device_number/issafe", get(get_is_safe))
        
        .layer(CorsLayer::permissive())
        .with_state((device_state, connection_manager))
}

type AppState = (SharedState, SharedConnectionManager);

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

async fn api_status(State((device_state, _)): State<AppState>) -> Json<DeviceState> {
    let device_state = device_state.read().await;
    Json(device_state.clone())
}

async fn api_ports() -> Json<PortListResponse> {
    match crate::port_discovery::discover_ports() {
        Ok(ports) => Json(PortListResponse { ports }),
        Err(_) => Json(PortListResponse { ports: vec![] }),
    }
}

async fn api_connect(
    State((_device_state, connection_manager)): State<AppState>,
    Json(request): Json<ConnectRequest>,
) -> Json<ConnectResponse> {
    info!("Connecting to port: {}", request.port);
    
    match connection_manager.connect(request.port.clone(), request.baud_rate.unwrap_or(115200)).await {
        Ok(_) => {
            info!("Successfully connected to {}", request.port);
            Json(ConnectResponse {
                success: true,
                message: format!("Connected to {}", request.port),
            })
        }
        Err(e) => {
            error!("Failed to connect to {}: {}", request.port, e);
            Json(ConnectResponse {
                success: false,
                message: format!("Failed to connect: {}", e),
            })
        }
    }
}

async fn api_disconnect(
    State((_device_state, connection_manager)): State<AppState>,
) -> Json<ConnectResponse> {
    info!("Disconnecting from device");
    
    connection_manager.disconnect().await;
    
    Json(ConnectResponse {
        success: true,
        message: "Disconnected".to_string(),
    })
}

async fn api_send_command(
    State((_device_state, connection_manager)): State<AppState>,
    Json(request): Json<CommandRequest>,
) -> Json<CommandResponse> {
    info!("Sending manual command: {}", request.command);
    
    match connection_manager.send_command(&request.command).await {
        Ok(response) => {
            Json(CommandResponse {
                success: true,
                command: request.command.clone(),
                response: Some(response),
                message: "Command sent successfully".to_string(),
            })
        }
        Err(e) => {
            error!("Failed to send command {}: {}", request.command, e);
            Json(CommandResponse {
                success: false,
                command: request.command.clone(),
                response: None,
                message: format!("Failed to send command: {}", e),
            })
        }
    }
}

async fn api_calibrate(
    State((_device_state, connection_manager)): State<AppState>,
) -> Json<CommandResponse> {
    info!("Starting sensor calibration");
    
    match connection_manager.send_command("06").await {
        Ok(response) => {
            Json(CommandResponse {
                success: true,
                command: "06".to_string(),
                response: Some(response),
                message: "Calibration command sent".to_string(),
            })
        }
        Err(e) => {
            error!("Failed to send calibration command: {}", e);
            Json(CommandResponse {
                success: false,
                command: "06".to_string(),
                response: None,
                message: format!("Failed to calibrate: {}", e),
            })
        }
    }
}

async fn api_set_park(
    State((_device_state, connection_manager)): State<AppState>,
) -> Json<CommandResponse> {
    info!("Setting park position");
    
    match connection_manager.send_command("04").await {
        Ok(response) => {
            Json(CommandResponse {
                success: true,
                command: "04".to_string(),
                response: Some(response),
                message: "Set park position command sent".to_string(),
            })
        }
        Err(e) => {
            error!("Failed to send set park command: {}", e);
            Json(CommandResponse {
                success: false,
                command: "04".to_string(),
                response: None,
                message: format!("Failed to set park position: {}", e),
            })
        }
    }
}

async fn api_factory_reset(
    State((_device_state, connection_manager)): State<AppState>,
) -> Json<CommandResponse> {
    info!("Performing factory reset");
    
    match connection_manager.send_command("0E").await {
        Ok(response) => {
            Json(CommandResponse {
                success: true,
                command: "0E".to_string(),
                response: Some(response),
                message: "Factory reset command sent".to_string(),
            })
        }
        Err(e) => {
            error!("Failed to send factory reset command: {}", e);
            Json(CommandResponse {
                success: false,
                command: "0E".to_string(),
                response: None,
                message: format!("Failed to factory reset: {}", e),
            })
        }
    }
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
    device.insert("UniqueID".to_string(), serde_json::Value::String("telescope-park-bridge-0".to_string()));
    
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

// ASCOM Safety Monitor API handlers - Fixed for v0.4.0
async fn get_connected(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State((device_state, _)): State<AppState>,
) -> Json<AlpacaResponse<bool>> {
    debug!("GET Connected called for device {}", device_number);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    let state = device_state.read().await;
    
    // For ASCOM, Connected represents client connection state
    Json(AlpacaResponse::success(
        state.ascom_connected,
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

// Simple PUT handler for connected property (ASCOM requirement)
async fn put_connected_simple(
    Path(device_number): Path<u32>,
    State((device_state, _)): State<AppState>,
) -> Json<AlpacaResponse<()>> {
    debug!("PUT Connected called for device {}", device_number);
    
    if device_number != 0 {
        return Json(AlpacaResponse::error(
            (),
            0,
            next_server_transaction_id(),
            0x400,
            "Invalid device number".to_string(),
        ));
    }
    
    // Toggle ASCOM connection state (connect on first call, disconnect on second)
    {
        let mut state = device_state.write().await;
        state.ascom_connected = !state.ascom_connected;
    }
    
    debug!("ASCOM client connection toggled");
    
    Json(AlpacaResponse::success(
        (),
        0,
        next_server_transaction_id(),
    ))
}

async fn get_description(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_): State<AppState>,
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
    State((device_state, _)): State<AppState>,
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
    
    let device_state = device_state.read().await;
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
    State(_): State<AppState>,
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
    State(_): State<AppState>,
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
        1, // Safety Monitor interface version
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_name(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_): State<AppState>,
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
        "nRF52840 Telescope Park Sensor".to_string(),
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_supported_actions(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_): State<AppState>,
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
        vec!["Calibrate".to_string(), "SetPark".to_string(), "FactoryReset".to_string()],
        query.client_transaction_id.unwrap_or(0),
        next_server_transaction_id(),
    ))
}

async fn get_is_safe(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State((device_state, connection_manager)): State<AppState>,
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
    
    // If not connected, report false (not safe)
    if !connection_manager.is_connected().await {
        return Json(AlpacaResponse::success(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
        ));
    }
    
    // Query the device for current park status
    match connection_manager.send_command("03").await {
        Ok(response) => {
            // Parse JSON response to get parked status
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&response) {
                if let Some(status) = parsed.get("status") {
                    if status == "ok" {
                        if let Some(data) = parsed.get("data") {
                            if let Some(parked) = data.get("parked") {
                                if let Some(is_parked) = parked.as_bool() {
                                    return Json(AlpacaResponse::success(
                                        is_parked,
                                        query.client_transaction_id.unwrap_or(0),
                                        next_server_transaction_id(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            
            // Fallback: check the device state
            let device_state = device_state.read().await;
            Json(AlpacaResponse::success(
                device_state.is_parked,
                query.client_transaction_id.unwrap_or(0),
                next_server_transaction_id(),
            ))
        }
        Err(e) => {
            error!("Failed to query park status: {}", e);
            Json(AlpacaResponse::error(
                false,
                query.client_transaction_id.unwrap_or(0),
                next_server_transaction_id(),
                0x500,
                format!("Failed to query device: {}", e),
            ))
        }
    }
}