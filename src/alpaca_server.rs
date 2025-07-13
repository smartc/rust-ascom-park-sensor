// src/alpaca_server.rs
// Fixed version with proper ClientTransactionID handling and PUT endpoints

use crate::device_state::DeviceState;
use axum::{
    extract::{Path, Query, State, Extension},
    response::{Html, Json},
    routing::{get, put},
    middleware,
    Router,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
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

// Form data structure for middleware
#[derive(Clone, Debug)]
struct ConnectedFormData {
    client_transaction_id: u32,
    connected: String,
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

// Form data for PUT requests
#[derive(Deserialize)]
struct ConnectedForm {
    #[serde(rename = "Connected")]
    #[serde(alias = "connected")]
    connected: String,
    
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

// Middleware to parse form data for PUT Connected requests
async fn parse_connected_form(
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    // Only process PUT requests to connected endpoint
    if request.method() == axum::http::Method::PUT && 
       request.uri().path().contains("/connected") {
        
        let (mut parts, body) = request.into_parts();
        let body_result = axum::body::to_bytes(body, usize::MAX).await;
        
        if let Ok(body_bytes) = body_result {
            let body_str = String::from_utf8_lossy(&body_bytes);
            
            let mut client_transaction_id = 0u32;
            let mut connected = String::new();
            
            // Parse form data manually since axum::extract::Form doesn't work in middleware
            for pair in body_str.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    match key {
                        "ClientTransactionID" | "clienttransactionid" | "ClientTransactionId" | "clientTransactionID" => {
                            if let Ok(decoded) = urlencoding::decode(value) {
                                client_transaction_id = decoded.parse().unwrap_or(0);
                            }
                        }
                        "Connected" | "connected" => {
                            if let Ok(decoded) = urlencoding::decode(value) {
                                connected = decoded.into_owned();
                            }
                        }
                        _ => {}
                    }
                }
            }
            
            // Insert parsed form data into request extensions
            parts.extensions.insert(Some(ConnectedFormData {
                client_transaction_id,
                connected,
            }));
            
            // Reconstruct request with original body
            let new_request = axum::http::Request::from_parts(parts, axum::body::Body::from(body_bytes));
            next.run(new_request).await
        } else {
            // If body reading failed, continue with empty body
            let new_request = axum::http::Request::from_parts(parts, axum::body::Body::empty());
            next.run(new_request).await
        }
    } else {
        next.run(request).await
    }
}

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
        .route("/api/v1/safetymonitor/:device_number/connected", put(put_connected))
        .route("/api/v1/safetymonitor/:device_number/description", get(get_description))
        .route("/api/v1/safetymonitor/:device_number/driverinfo", get(get_driver_info))
        .route("/api/v1/safetymonitor/:device_number/driverversion", get(get_driver_version))
        .route("/api/v1/safetymonitor/:device_number/interfaceversion", get(get_interface_version))
        .route("/api/v1/safetymonitor/:device_number/name", get(get_name))
        .route("/api/v1/safetymonitor/:device_number/supportedactions", get(get_supported_actions))
        
        // ASCOM Device API - SafetyMonitor specific
        .route("/api/v1/safetymonitor/:device_number/issafe", get(get_is_safe))
        
        .layer(middleware::from_fn(parse_connected_form))
        .layer(CorsLayer::permissive())
        .with_state(device_state)
}

// Helper function to extract client transaction ID with proper default handling
fn get_client_transaction_id(query_id: Option<u32>) -> u32 {
    query_id.unwrap_or(0)
}

// Validation function for device number
fn validate_device_number(device_number: u32, client_transaction_id: u32) -> Result<(), Json<AlpacaResponse<serde_json::Value>>> {
    if device_number != 0 {
        return Err(Json(AlpacaResponse::error(
            serde_json::Value::Null,
            client_transaction_id,
            1024, // ASCOM error code for invalid device number
            format!("Invalid device number: {}", device_number),
        )));
    }
    Ok(())
}

// Web interface handlers
async fn web_interface() -> Html<String> {
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS);
    
    Html(html)
}

async fn web_interface_device_control(Path(device_number): Path<u32>) -> Html<String> {
    if device_number != 0 {
        return Html("<h1>Error: Invalid device number. Only device 0 is supported.</h1>".to_string());
    }
    
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS);
    
    Html(html)
}

// API handlers for web interface
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
    // Implementation depends on your serial connection logic
    Json(ConnectResponse {
        success: true,
        message: format!("Connecting to {}", request.port),
    })
}

async fn api_disconnect(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    // Implementation depends on your serial connection logic
    Json(ConnectResponse {
        success: true,
        message: "Disconnected".to_string(),
    })
}

async fn api_send_command(
    State(_state): State<SharedState>,
    Json(request): Json<CommandRequest>,
) -> Json<CommandResponse> {
    // Implementation depends on your command sending logic
    Json(CommandResponse {
        success: true,
        command: request.command,
        response: Some("OK".to_string()),
        message: "Command sent".to_string(),
    })
}

async fn api_calibrate(State(_state): State<SharedState>) -> Json<CommandResponse> {
    Json(CommandResponse {
        success: true,
        command: "CALIBRATE".to_string(),
        response: Some("OK".to_string()),
        message: "Calibration initiated".to_string(),
    })
}

async fn api_set_park(State(_state): State<SharedState>) -> Json<CommandResponse> {
    Json(CommandResponse {
        success: true,
        command: "SET_PARK".to_string(),
        response: Some("OK".to_string()),
        message: "Park position set".to_string(),
    })
}

async fn api_factory_reset(State(_state): State<SharedState>) -> Json<CommandResponse> {
    Json(CommandResponse {
        success: true,
        command: "FACTORY_RESET".to_string(),
        response: Some("OK".to_string()),
        message: "Factory reset initiated".to_string(),
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
) -> Result<Json<AlpacaResponse<bool>>, (StatusCode, Json<AlpacaResponse<bool>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                false,
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    let device_state = state.read().await;
    Ok(Json(AlpacaResponse::success(device_state.ascom_connected, client_transaction_id)))
}

// PUT Connected handler with proper parameter validation
async fn put_connected(
    Path(device_number): Path<u32>,
    Extension(form_data): Extension<Option<ConnectedFormData>>,
    State(state): State<SharedState>,
) -> Result<Json<AlpacaResponse<()>>, (StatusCode, Json<AlpacaResponse<()>>)> {
    let client_transaction_id = form_data.as_ref().map(|d| d.client_transaction_id).unwrap_or(0);
    
    // Validate device number
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                (),
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    // Validate form data exists
    let form_data = match form_data {
        Some(data) => data,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AlpacaResponse::error(
                    (),
                    client_transaction_id,
                    1025,
                    "Missing form data".to_string(),
                ))
            ));
        }
    };
    
    // Validate Connected parameter
    let connected_value = match form_data.connected.to_lowercase().as_str() {
        "true" => true,
        "false" => false,
        "" => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AlpacaResponse::error(
                    (),
                    client_transaction_id,
                    1026,
                    "Empty Connected parameter".to_string(),
                ))
            ));
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AlpacaResponse::error(
                    (),
                    client_transaction_id,
                    1027,
                    "Invalid Connected parameter - must be 'true' or 'false'".to_string(),
                ))
            ));
        }
    };
    
    // Update device state
    {
        let mut device_state = state.write().await;
        device_state.ascom_connected = connected_value;
        info!("ASCOM Connected set to: {}", connected_value);
    }
    
    Ok(Json(AlpacaResponse::success((), client_transaction_id)))
}

async fn get_description(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<SharedState>,
) -> Result<Json<AlpacaResponse<String>>, (StatusCode, Json<AlpacaResponse<String>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                String::new(),
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    Ok(Json(AlpacaResponse::success(
        "nRF52840 Based Custom Position Sensor for Telescope Park Detection".to_string(),
        client_transaction_id,
    )))
}

async fn get_driver_info(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Result<Json<AlpacaResponse<String>>, (StatusCode, Json<AlpacaResponse<String>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                String::new(),
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    let device_state = state.read().await;
    let driver_info = format!("Telescope Park Bridge v{} for {}", 
        env!("CARGO_PKG_VERSION"), 
        device_state.device_name
    );
    
    Ok(Json(AlpacaResponse::success(driver_info, client_transaction_id)))
}

async fn get_driver_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<SharedState>,
) -> Result<Json<AlpacaResponse<String>>, (StatusCode, Json<AlpacaResponse<String>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                String::new(),
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    Ok(Json(AlpacaResponse::success(
        env!("CARGO_PKG_VERSION").to_string(),
        client_transaction_id,
    )))
}

async fn get_interface_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<SharedState>,
) -> Result<Json<AlpacaResponse<u32>>, (StatusCode, Json<AlpacaResponse<u32>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                0,
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    Ok(Json(AlpacaResponse::success(1, client_transaction_id)))
}

async fn get_name(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Result<Json<AlpacaResponse<String>>, (StatusCode, Json<AlpacaResponse<String>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                String::new(),
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    let device_state = state.read().await;
    Ok(Json(AlpacaResponse::success(device_state.device_name.clone(), client_transaction_id)))
}

async fn get_supported_actions(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<SharedState>,
) -> Result<Json<AlpacaResponse<Vec<String>>>, (StatusCode, Json<AlpacaResponse<Vec<String>>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                vec![],
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    Ok(Json(AlpacaResponse::success(
        vec![], // No custom actions supported
        client_transaction_id,
    )))
}

async fn get_is_safe(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<SharedState>,
) -> Result<Json<AlpacaResponse<bool>>, (StatusCode, Json<AlpacaResponse<bool>>)> {
    let client_transaction_id = get_client_transaction_id(query.client_transaction_id);
    
    if device_number != 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AlpacaResponse::error(
                false,
                client_transaction_id,
                1024,
                format!("Invalid device number: {}", device_number),
            ))
        ));
    }
    
    let device_state = state.read().await;
    
    // If not connected, it's not safe
    if !device_state.connected {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AlpacaResponse::error(
                false,
                client_transaction_id,
                1032, // ASCOM error code for not connected
                "Device not connected".to_string(),
            ))
        ));
    }
    
    // Check if data is recent (within last 30 seconds)
    if !device_state.is_recent(30) {
        return Err((
            StatusCode::REQUEST_TIMEOUT,
            Json(AlpacaResponse::error(
                false,
                client_transaction_id,
                1033, // ASCOM error code for timeout
                "Device data is stale".to_string(),
            ))
        ));
    }
    
    Ok(Json(AlpacaResponse::success(device_state.is_safe, client_transaction_id)))
}