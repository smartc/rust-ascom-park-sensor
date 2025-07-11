
use crate::device_state::DeviceState;
use crate::telescope_client::{TelescopeClient, TelescopeConnection, SlewDirection};
use axum::{
    extract::{Path, Query, State},
    response::{Html, Json},
    routing::{get, post},
    Router, Json as ExtractJson,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tracing::info;

// Global to track the current serial connection task
use std::sync::Mutex;
use std::sync::OnceLock;

static SERIAL_TASK: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();
static TELESCOPE_CLIENT: OnceLock<Mutex<Option<TelescopeClient>>> = OnceLock::new();

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

// Single TelescopeConnectRequest definition
#[derive(Deserialize)]
struct TelescopeConnectRequest {
    connection_type: String,  // "alpaca" or "local"
    url: Option<String>,
    device_number: Option<u32>,
    prog_id: Option<String>,
}

#[derive(Deserialize)]
struct SlewRequest {
    ra: f64,
    dec: f64,
}

#[derive(Deserialize)]
struct ManualSlewRequest {
    direction: String,  // "north", "south", "east", "west"
    rate: Option<f64>,  // Slew rate (degrees per second)
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
struct TelescopeListResponse {
    telescopes: Vec<String>,
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
        
        // Telescope control routes
        .route("/api/telescope/connect", post(api_telescope_connect))
        .route("/api/telescope/disconnect", post(api_telescope_disconnect))
        .route("/api/telescope/slew", post(api_telescope_slew))
        .route("/api/telescope/abort", post(api_telescope_abort))
        .route("/api/telescope/tracking", post(api_telescope_tracking))
        .route("/api/telescope/park", post(api_telescope_park))
        .route("/api/telescope/unpark", post(api_telescope_unpark))
        .route("/api/telescope/home", post(api_telescope_home))
        .route("/api/telescope/list", get(api_telescope_list))
        .route("/api/telescope/slew/manual", post(api_telescope_manual_slew))
        .route("/api/telescope/slew/stop", post(api_telescope_stop_slew))
        .route("/api/telescope/axis_rates", get(api_telescope_axis_rates))

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

async fn api_connect(
    State(state): State<SharedState>,
    ExtractJson(request): ExtractJson<ConnectRequest>,
) -> Json<ConnectResponse> {
    let baud_rate = request.baud_rate.unwrap_or(115200);
    
    // Abort any existing serial task
    let task_mutex = SERIAL_TASK.get_or_init(|| Mutex::new(None));
    if let Ok(mut current_task) = task_mutex.lock() {
        if let Some(task) = current_task.take() {
            task.abort();
        }
    }
    
    // Start a new serial connection task
    let device_state_clone = state.clone();
    let port = request.port.clone();
    
    let new_task = tokio::spawn(async move {
        if let Err(e) = crate::serial_client::run_serial_client(port, baud_rate, device_state_clone).await {
            tracing::error!("Serial client error: {}", e);
        }
    });
    
    // Store the new task
    if let Ok(mut current_task) = task_mutex.lock() {
        *current_task = Some(new_task);
    }
    
    // Update the device state to show the selected port
    {
        let mut device_state = state.write().await;
        device_state.serial_port = Some(request.port.clone());
        device_state.clear_error();
    }
    
    Json(ConnectResponse {
        success: true,
        message: format!("Connecting to {} at {} baud", request.port, baud_rate),
    })
}

async fn api_disconnect(State(state): State<SharedState>) -> Json<ConnectResponse> {
    // Abort the current serial task
    let task_mutex = SERIAL_TASK.get_or_init(|| Mutex::new(None));
    if let Ok(mut current_task) = task_mutex.lock() {
        if let Some(task) = current_task.take() {
            task.abort();
        }
    }
    
    // Update device state to disconnected
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

// Telescope API handlers
async fn api_telescope_connect(
    State(state): State<SharedState>,
    ExtractJson(request): ExtractJson<TelescopeConnectRequest>,
) -> Json<ConnectResponse> {
    let connection = match request.connection_type.as_str() {
        "alpaca" => {
            let url = match request.url {
                Some(url) => url,
                None => {
                    return Json(ConnectResponse {
                        success: false,
                        message: "URL required for Alpaca connection".to_string(),
                    });
                }
            };
            let device_number = request.device_number.unwrap_or(0);
            
            tracing::info!("Connecting to Alpaca telescope at {} device {}", url, device_number);
            TelescopeConnection::Alpaca { url, device_number }
        }
        "local" => {
            let prog_id = match request.prog_id {
                Some(prog_id) => prog_id,
                None => {
                    return Json(ConnectResponse {
                        success: false,
                        message: "ProgID required for local connection".to_string(),
                    });
                }
            };
            
            tracing::info!("Connecting to local ASCOM telescope: {}", prog_id);
            TelescopeConnection::Local { prog_id }
        }
        _ => {
            return Json(ConnectResponse {
                success: false,
                message: "Invalid connection type. Use 'alpaca' or 'local'".to_string(),
            });
        }
    };
    
    let mut client = TelescopeClient::new(connection);
    
    // Test connection
    match client.connect().await {
        Ok(()) => {
            // Store the client
            let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
            if let Ok(mut current_client) = client_mutex.lock() {
                *current_client = Some(client);
            }
            
            // Update device state
            {
                let mut device_state = state.write().await;
                device_state.telescope_connected = true;
            }
            
            // Start telescope status monitoring
            let state_clone = state.clone();
            tokio::spawn(async move {
                telescope_status_monitor(state_clone).await;
            });
            
            Json(ConnectResponse {
                success: true,
                message: "Connected to telescope".to_string(),
            })
        }
        Err(e) => {
            Json(ConnectResponse {
                success: false,
                message: format!("Failed to connect to telescope: {}", e),
            })
        }
    }
}

async fn api_telescope_disconnect(State(state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Disconnecting from telescope");
    
    // Get client and disconnect - using clone pattern to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(mut current_client) = client_mutex.lock() {
            current_client.take()
        } else {
            None
        }
    };
    
    let result = if let Some(mut client) = client_option {
        client.disconnect().await
    } else {
        Ok(())
    };
    
    // Update device state
    {
        let mut device_state = state.write().await;
        device_state.telescope_connected = false;
        device_state.telescope_url = None;
    }
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "Disconnected from telescope".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Error disconnecting: {}", e),
        }),
    }
}

async fn api_telescope_slew(
    State(_state): State<SharedState>,
    ExtractJson(request): ExtractJson<SlewRequest>,
) -> Json<ConnectResponse> {
    tracing::info!("Slewing telescope to RA: {}, Dec: {}", request.ra, request.dec);
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.slew_to_coordinates(request.ra, request.dec).await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: format!("Slewing to RA: {}, Dec: {}", request.ra, request.dec),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Slew failed: {}", e),
        }),
    }
}

async fn api_telescope_manual_slew(
    State(_state): State<SharedState>,
    ExtractJson(request): ExtractJson<ManualSlewRequest>,
) -> Json<ConnectResponse> {
    let direction = match request.direction.to_lowercase().as_str() {
        "north" => SlewDirection::North,
        "south" => SlewDirection::South,
        "east" => SlewDirection::East,
        "west" => SlewDirection::West,
        _ => {
            return Json(ConnectResponse {
                success: false,
                message: "Invalid direction. Use: north, south, east, or west".to_string(),
            });
        }
    };
    
    let rate = request.rate.unwrap_or(1.0); // Default 1 degree/second
    
    tracing::info!("Manual slew {:?} at rate {}", direction, rate);
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.move_axis(direction, rate).await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: format!("Moving {:?} at {} deg/s", direction, rate),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Manual slew failed: {}", e),
        }),
    }
}

async fn api_telescope_stop_slew(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Stopping all telescope movement");
    
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.stop_all_movement().await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "All telescope movement stopped".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Stop failed: {}", e),
        }),
    }
}

async fn api_telescope_axis_rates(State(_state): State<SharedState>) -> Json<serde_json::Value> {
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let rates = if let Some(client) = client_option {
        client.get_axis_rates().await.unwrap_or_else(|_| vec![0.5, 1.0, 2.0, 4.0])
    } else {
        vec![0.5, 1.0, 2.0, 4.0] // Default rates
    };
    
    Json(serde_json::json!({
        "rates": rates
    }))
}

async fn api_telescope_abort(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Aborting telescope slew");
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.abort_slew().await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "Slew aborted".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Abort failed: {}", e),
        }),
    }
}

async fn api_telescope_tracking(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Toggling telescope tracking");
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        // Get current tracking state and toggle it
        match client.get_status().await {
            Ok(status) => client.set_tracking(!status.tracking).await,
            Err(e) => Err(e),
        }
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "Tracking toggled".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Tracking toggle failed: {}", e),
        }),
    }
}

async fn api_telescope_park(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Parking telescope");
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.park().await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "Telescope parking".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Park failed: {}", e),
        }),
    }
}

async fn api_telescope_unpark(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Unparking telescope");
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.unpark().await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "Telescope unparking".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Unpark failed: {}", e),
        }),
    }
}

async fn api_telescope_home(State(_state): State<SharedState>) -> Json<ConnectResponse> {
    tracing::info!("Finding telescope home");
    
    // Get a clone of the client to avoid holding guard across await
    let client_option = {
        let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
        if let Ok(current_client) = client_mutex.lock() {
            current_client.as_ref().cloned()
        } else {
            None
        }
    };
    
    let result = if let Some(client) = client_option {
        client.find_home().await
    } else {
        Err("No telescope connected".into())
    };
    
    match result {
        Ok(()) => Json(ConnectResponse {
            success: true,
            message: "Telescope finding home".to_string(),
        }),
        Err(e) => Json(ConnectResponse {
            success: false,
            message: format!("Find home failed: {}", e),
        }),
    }
}

async fn api_telescope_list() -> Json<TelescopeListResponse> {
    match crate::telescope_client::discover_local_ascom_telescopes() {
        Ok(telescopes) => {
            tracing::info!("Found {} local ASCOM telescopes", telescopes.len());
            Json(TelescopeListResponse { telescopes })
        }
        Err(e) => {
            tracing::warn!("Failed to discover local telescopes: {}", e);
            Json(TelescopeListResponse { telescopes: vec![] })
        }
    }
}

// Telescope status monitoring background task
async fn telescope_status_monitor(device_state: SharedState) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3));
    
    loop {
        interval.tick().await;
        
        // Get the client outside the async block to avoid holding the mutex across await
        let client_option = {
            let client_mutex = TELESCOPE_CLIENT.get_or_init(|| Mutex::new(None));
            if let Ok(current_client) = client_mutex.lock() {
                current_client.as_ref().cloned()
            } else {
                None
            }
        };
        
        if let Some(client) = client_option {
            match client.get_status().await {
                Ok(telescope_status) => {
                    let mut state = device_state.write().await;
                    state.telescope_status = telescope_status;
                    state.update_timestamp();
                }
                Err(_) => {
                    // Lost connection to telescope
                    let mut state = device_state.write().await;
                    if state.telescope_connected {
                        state.telescope_connected = false;
                        tracing::warn!("Lost connection to telescope");
                    }
                    break;
                }
            }
        } else {
            // No telescope client available
            let mut state = device_state.write().await;
            if state.telescope_connected {
                state.telescope_connected = false;
                tracing::warn!("Telescope client not available");
            }
            break;
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
        "ESP32 Based Custom Position Sensor for Telescope Park Detection".to_string(),
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