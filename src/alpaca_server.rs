// src/alpaca_server.rs
// Fixed version with proper ClientTransactionID handling and PUT endpoints

use crate::device_state::DeviceState;
use crate::connection_manager::ConnectionManager;
use axum::{
    extract::{Path, Query, State, Extension},
    response::{Html, Json, Response},  // Add Response
    routing::{get, put},
    middleware,
    Router,
    http::{StatusCode, HeaderMap, HeaderValue, header},
    body::Body,
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
const ICON_PNG: &[u8] = include_bytes!("../assets/telescope-icon.png");

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

// Updated SharedState to include ConnectionManager
#[derive(Clone)]
struct AppState {
    device_state: Arc<RwLock<DeviceState>>,
    connection_manager: Arc<ConnectionManager>,
}

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
    device_state: Arc<RwLock<DeviceState>>,
    connection_manager: Arc<ConnectionManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app_state = AppState {
        device_state,
        connection_manager,
    };
    
    let app = create_router(app_state);
    
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_address, port)).await?;
    
    info!("ASCOM Alpaca server listening on {}:{}", bind_address, port);
    
    axum::serve(listener, app).await?;
    Ok(())
}

fn create_router(app_state: AppState) -> Router {
    Router::new()
        // Web interface
        .route("/", get(web_interface))

        // Web icon routes
        .route("/favicon.ico", get(serve_favicon))
        .route("/icon-192.png", get(serve_icon_192))
        .route("/icon-512.png", get(serve_icon_512))
        
        // Device setup endpoints
        .route("/setup", get(web_interface))
        .route("/setup/v1/safetymonitor/:device_number/setup", get(web_interface_device_control))
        
        // Web API endpoints
        .route("/api/status", get(api_status))
        .route("/api/ports", get(api_ports))
        .route("/api/connect", axum::routing::post(api_connect))
        .route("/api/disconnect", axum::routing::post(api_disconnect))
        .route("/api/command", axum::routing::post(api_send_command))
        .route("/api/device/calibrate", axum::routing::post(api_calibrate))
        .route("/api/device/set_park", axum::routing::post(api_set_park))
        .route("/api/device/factory_reset", axum::routing::post(api_factory_reset))
        
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
        .with_state(app_state)
}

// Helper function to extract client transaction ID with proper default handling
fn get_client_transaction_id(query_id: Option<u32>) -> u32 {
    query_id.unwrap_or(0)
}

// Web interface handlers
async fn web_interface() -> Html<String> {
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS)
        .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
        .replace("{{BUILD}}", env!("BUILD_TIMESTAMP"));
    
    Html(html)
}

async fn web_interface_device_control(Path(device_number): Path<u32>) -> Html<String> {
    if device_number != 0 {
        return Html("<h1>Error: Invalid device number. Only device 0 is supported.</h1>".to_string());
    }
    
    let html = INDEX_HTML
        .replace("{{STYLE_CSS}}", STYLE_CSS)
        .replace("{{SCRIPT_JS}}", SCRIPT_JS)
        .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
        .replace("{{BUILD}}", env!("BUILD_TIMESTAMP"));
    
    Html(html)
}

// API handlers for web interface - UNSTUBBED to use ConnectionManager
async fn api_status(State(state): State<AppState>) -> Json<DeviceState> {
    let device_state = state.device_state.read().await;
    Json(device_state.clone())
}

async fn api_ports() -> Json<PortListResponse> {
    match crate::port_discovery::discover_ports() {
        Ok(ports) => Json(PortListResponse { ports }),
        Err(_) => Json(PortListResponse { ports: vec![] }),
    }
}

async fn api_connect(
    State(state): State<AppState>,
    Json(request): Json<ConnectRequest>,
) -> Json<ConnectResponse> {
    let baud_rate = request.baud_rate.unwrap_or(115200);
    
    match state.connection_manager.connect(request.port.clone(), baud_rate).await {
        Ok(message) => {
            info!("Connection successful: {}", message);
            Json(ConnectResponse {
                success: true,
                message,
            })
        }
        Err(e) => {
            let error_msg = format!("Failed to connect: {}", e);
            info!("Connection failed: {}", error_msg);
            Json(ConnectResponse {
                success: false,
                message: error_msg,
            })
        }
    }
}

async fn api_disconnect(State(state): State<AppState>) -> Json<ConnectResponse> {
    match state.connection_manager.disconnect().await {
        Ok(message) => {
            info!("Disconnection successful: {}", message);
            Json(ConnectResponse {
                success: true,
                message,
            })
        }
        Err(e) => {
            let error_msg = format!("Failed to disconnect: {}", e);
            info!("Disconnection failed: {}", error_msg);
            Json(ConnectResponse {
                success: false,
                message: error_msg,
            })
        }
    }
}

async fn api_send_command(
    State(state): State<AppState>,
    Json(request): Json<CommandRequest>,
) -> Json<CommandResponse> {
    match state.connection_manager.send_command(&request.command).await {
        Ok(response) => {
            info!("Command '{}' executed successfully", request.command);
            Json(CommandResponse {
                success: true,
                command: request.command,
                response: Some(response),
                message: "Command executed successfully".to_string(),
            })
        }
        Err(e) => {
            let error_msg = format!("Command failed: {}", e);
            info!("Command '{}' failed: {}", request.command, error_msg);
            Json(CommandResponse {
                success: false,
                command: request.command,
                response: None,
                message: error_msg,
            })
        }
    }
}

async fn api_calibrate(State(state): State<AppState>) -> Json<CommandResponse> {
    match state.connection_manager.calibrate_sensor().await {
        Ok(response) => {
            info!("Sensor calibration completed successfully");
            Json(CommandResponse {
                success: true,
                command: "06".to_string(),
                response: Some(response),
                message: "Sensor calibration completed".to_string(),
            })
        }
        Err(e) => {
            let error_msg = format!("Calibration failed: {}", e);
            info!("Sensor calibration failed: {}", error_msg);
            Json(CommandResponse {
                success: false,
                command: "06".to_string(),
                response: None,
                message: error_msg,
            })
        }
    }
}

async fn api_set_park(State(state): State<AppState>) -> Json<CommandResponse> {
    match state.connection_manager.set_park_position().await {
        Ok(response) => {
            info!("Park position set successfully");
            Json(CommandResponse {
                success: true,
                command: "0D".to_string(),
                response: Some(response),
                message: "Park position set successfully".to_string(),
            })
        }
        Err(e) => {
            let error_msg = format!("Set park failed: {}", e);
            info!("Set park position failed: {}", error_msg);
            Json(CommandResponse {
                success: false,
                command: "0D".to_string(),
                response: None,
                message: error_msg,
            })
        }
    }
}

async fn api_factory_reset(State(state): State<AppState>) -> Json<CommandResponse> {
    match state.connection_manager.factory_reset().await {
        Ok(response) => {
            info!("Factory reset completed successfully");
            Json(CommandResponse {
                success: true,
                command: "0E".to_string(),
                response: Some(response),
                message: "Factory reset completed".to_string(),
            })
        }
        Err(e) => {
            let error_msg = format!("Factory reset failed: {}", e);
            info!("Factory reset failed: {}", error_msg);
            Json(CommandResponse {
                success: false,
                command: "0E".to_string(),
                response: None,
                message: error_msg,
            })
        }
    }
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
    State(state): State<AppState>
) -> Json<AlpacaResponse<Vec<serde_json::Value>>> {
    let device_state = state.device_state.read().await;
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
    State(state): State<AppState>,
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
    
    let device_state = state.device_state.read().await;
    Ok(Json(AlpacaResponse::success(device_state.ascom_connected, client_transaction_id)))
}

// PUT Connected handler with proper parameter validation
async fn put_connected(
    Path(device_number): Path<u32>,
    Extension(form_data): Extension<Option<ConnectedFormData>>,
    State(state): State<AppState>,
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
        let mut device_state = state.device_state.write().await;
        device_state.ascom_connected = connected_value;
        info!("ASCOM Connected set to: {}", connected_value);
    }
    
    Ok(Json(AlpacaResponse::success((), client_transaction_id)))
}

async fn get_description(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<AppState>,
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
        "nRF52840 based telescope park position sensor for ASCOM safety monitoring".to_string(),
        client_transaction_id,
    )))
}

async fn get_driver_info(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<AppState>,
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
    
    let device_state = state.device_state.read().await;
    let driver_info = format!("nRF52840 Telescope Park Bridge v{} for {}", 
        env!("CARGO_PKG_VERSION"), device_state.device_name);
    
    Ok(Json(AlpacaResponse::success(
        driver_info,
        client_transaction_id,
    )))
}

async fn get_driver_version(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<AppState>,
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
    State(_state): State<AppState>,
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
    State(state): State<AppState>,
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
    
    let device_state = state.device_state.read().await;
    Ok(Json(AlpacaResponse::success(
        device_state.device_name.clone(),
        client_transaction_id,
    )))
}

async fn get_supported_actions(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(_state): State<AppState>,
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
    
    Ok(Json(AlpacaResponse::success(vec![], client_transaction_id)))
}

async fn get_is_safe(
    Path(device_number): Path<u32>,
    Query(query): Query<AlpacaQuery>,
    State(state): State<AppState>,
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
    
    let device_state = state.device_state.read().await;
    
    // ASCOM compliance: IsSafe should return false if not connected
    let is_safe = if device_state.connected {
        device_state.is_safe
    } else {
        false
    };
    
    Ok(Json(AlpacaResponse::success(
        is_safe,
        client_transaction_id,
    )))
}

async fn serve_favicon() -> Response<Body> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(ICON_PNG))
        .unwrap()
}

async fn serve_icon_192() -> Response<Body> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(ICON_PNG))
        .unwrap()
}

async fn serve_icon_512() -> Response<Body> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(ICON_PNG))
        .unwrap()
}
