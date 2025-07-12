use crate::device_state::DeviceState;
use crate::connection_manager::ConnectionManager;
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
use tower_http::cors::CorsLayer;
use tracing::{info, error};

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
    State((_, connection_manager)): State<AppState>,
    Json(request): Json<ConnectRequest>,
) -> Json<ConnectResponse> {
    let baud_rate = request.baud_rate.unwrap_or(115200);
    
    match connection_manager.connect(request.port, baud_rate).await {
        Ok(message) => Json(ConnectResponse {
            success: true,
            message,
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Connection failed: {}", e),
        }),
    }
}

async fn api_disconnect(State((_, connection_manager)): State<AppState>) -> Json<ConnectResponse> {
    match connection_manager.disconnect().await {
        Ok(message) => Json(ConnectResponse {
            success: true,
            message,
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Disconnect failed: {}", e),
        }),
    }
}

async fn api_send_command(
    State((_, connection_manager)): State<AppState>, 
    Json(request): Json<CommandRequest>
) -> Json<CommandResponse> {
    info!("API: Sending manual command: {}", request.command);
    
    if !connection_manager.is_connected().await {
        return Json(CommandResponse {
            success: false,
            command: request.command,
            response: None,
            message: "Device not connected".to_string(),
        });
    }
    
    // Validate command format (should be hex digits only)
    let command = request.command.trim().to_uppercase();
    if command.is_empty() || !command.chars().all(|c| c.is_ascii_hexdigit()) {
        return Json(CommandResponse {
            success: false,
            command: request.command,
            response: None,
            message: "Invalid command format. Use hex digits only (e.g., 01, 02, 0A050)".to_string(),
        });
    }
    
    match connection_manager.send_command(&command).await {
        Ok(response) => {
            info!("Command {} succeeded: {}", command, response);
            Json(CommandResponse {
                success: true,
                command: command,
                response: Some(response),
                message: "Command sent successfully".to_string(),
            })
        }
        Err(e) => {
            error!("Command {} failed: {}", command, e);
            Json(CommandResponse {
                success: false,
                command: command,
                response: None,
                message: format!("Command failed: {}", e),
            })
        }
    }
}

async fn api_calibrate(State((_, connection_manager)): State<AppState>) -> Json<ConnectResponse> {
    info!("API: Starting sensor calibration");
    
    if !connection_manager.is_connected().await {
        return Json(ConnectResponse {
            success: false,
            message: "Device not connected".to_string(),
        });
    }
    
    match connection_manager.calibrate_sensor().await {
        Ok(response) => {
            info!("Calibration succeeded: {}", response);
            Json(ConnectResponse {
                success: true,
                message: "IMU calibration started successfully. Keep device still during calibration.".to_string(),
            })
        }
        Err(e) => {
            error!("Calibration failed: {}", e);
            Json(ConnectResponse {
                success: false,
                message: format!("Calibration failed: {}", e),
            })
        }
    }
}

async fn api_set_park(State((_, connection_manager)): State<AppState>) -> Json<ConnectResponse> {
    info!("API: Setting park position");
    
    if !connection_manager.is_connected().await {
        return Json(ConnectResponse {
            success: false,
            message: "Device not connected".to_string(),
        });
    }
    
    match connection_manager.set_park_position().await {
        Ok(response) => {
            info!("Set park position succeeded: {}", response);
            Json(ConnectResponse {
                success: true,
                message: "Park position set to current telescope position successfully.".to_string(),
            })
        }
        Err(e) => {
            error!("Set park position failed: {}", e);
            Json(ConnectResponse {
                success: false,
                message: format!("Failed to set park position: {}", e),
            })
        }
    }
}

async fn api_factory_reset(State((_, connection_manager)): State<AppState>) -> Json<ConnectResponse> {
    info!("API: Performing factory reset");
    
    if !connection_manager.is_connected().await {
        return Json(ConnectResponse {
            success: false,
            message: "Device not connected".to_string(),
        });
    }
    
    match connection_manager.factory_reset().await {
        Ok(response) => {
            info!("Factory reset succeeded: {}", response);
            Json(ConnectResponse {
                success: true,
                message: "Factory reset completed successfully. Device will restart and all settings have been cleared.".to_string(),
            })
        }
        Err(e) => {
            error!("Factory reset failed: {}", e);
            Json(ConnectResponse {
                success: false,
                message: format!("Factory reset failed: {}", e),
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
    State((_, connection_manager)): State<AppState>,
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
    
    let connected = connection_manager.is_connected().await;
    Json(AlpacaResponse::success(
        connected,
        query.client_transaction_id.unwrap_or(0),
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
    
    // Check if connected
    if !connection_manager.is_connected().await {
        return Json(AlpacaResponse::error(
            false,
            query.client_transaction_id.unwrap_or(0),
            next_server_transaction_id(),
            0x407,
            "nRF52840 device not connected".to_string(),
        ));
    }
    
    let device_state = device_state.read().await;
    
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