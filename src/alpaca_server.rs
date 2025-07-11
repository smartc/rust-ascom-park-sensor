use crate::device_state::DeviceState;
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

#[derive(Serialize)]
struct PortListResponse {
    ports: Vec<crate::port_discovery::PortInfo>,
}

#[derive(Serialize)]
struct ConnectResponse {
    success: bool,
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
async fn web_interface() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Telescope Park Bridge</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 900px; margin: 0 auto; background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        h1 { color: #2c3e50; border-bottom: 3px solid #3498db; padding-bottom: 10px; }
        .status { padding: 15px; margin: 15px 0; border-radius: 5px; }
        .connected { background: #d4edda; border: 1px solid #c3e6cb; color: #155724; }
        .disconnected { background: #f8d7da; border: 1px solid #f5c6cb; color: #721c24; }
        .safe { background: #d1ecf1; border: 1px solid #bee5eb; color: #0c5460; }
        .unsafe { background: #fff3cd; border: 1px solid #ffeaa7; color: #856404; }
        .info-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; margin: 20px 0; }
        .info-box { padding: 15px; background: #f8f9fa; border-radius: 5px; border-left: 4px solid #3498db; }
        .info-box h3 { margin-top: 0; color: #2c3e50; }
        .value { font-weight: bold; color: #27ae60; }
        .control-panel { background: #e9ecef; padding: 20px; border-radius: 5px; margin: 20px 0; }
        .control-panel h3 { margin-top: 0; }
        .endpoints { background: #e9ecef; padding: 20px; border-radius: 5px; margin: 20px 0; }
        .endpoints h3 { margin-top: 0; }
        .endpoint { font-family: monospace; background: white; padding: 8px; margin: 5px 0; border-radius: 3px; }
        button { background: #3498db; color: white; border: none; padding: 10px 20px; border-radius: 5px; cursor: pointer; margin: 5px; }
        button:hover { background: #2980b9; }
        button:disabled { background: #95a5a6; cursor: not-allowed; }
        .btn-success { background: #27ae60; }
        .btn-success:hover { background: #229954; }
        .btn-danger { background: #e74c3c; }
        .btn-danger:hover { background: #c0392b; }
        select, input { padding: 8px; margin: 5px; border: 1px solid #bdc3c7; border-radius: 3px; }
        .form-group { margin: 10px 0; }
        .form-group label { display: inline-block; width: 100px; }
        #log { background: #2c3e50; color: #ecf0f1; padding: 15px; border-radius: 5px; height: 200px; overflow-y: scroll; font-family: monospace; font-size: 12px; white-space: pre-wrap; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔭 Telescope Park Bridge</h1>
        
        <div id="connection-status" class="status disconnected">
            ⚠️ Checking connection...
        </div>
        
        <div id="safety-status" class="status unsafe">
            🚫 Safety status unknown
        </div>
        
        <div class="control-panel">
            <h3>Serial Port Control</h3>
            <div class="form-group">
                <label for="port-select">Port:</label>
                <select id="port-select">
                    <option value="">Loading ports...</option>
                </select>
                <button onclick="refreshPorts()">🔄 Refresh</button>
            </div>
            <div class="form-group">
                <label for="baud-rate">Baud Rate:</label>
                <input type="number" id="baud-rate" value="115200" min="9600" max="921600">
            </div>
            <div class="form-group">
                <button id="connect-btn" class="btn-success" onclick="connectToPort()">🔌 Connect</button>
                <button id="disconnect-btn" class="btn-danger" onclick="disconnectFromPort()" disabled>❌ Disconnect</button>
            </div>
        </div>
        
        <div class="info-grid">
            <div class="info-box">
                <h3>Device Information</h3>
                <p><strong>Name:</strong> <span id="device-name">Loading...</span></p>
                <p><strong>Version:</strong> <span id="device-version">Loading...</span></p>
                <p><strong>Manufacturer:</strong> <span id="manufacturer">Loading...</span></p>
                <p><strong>Serial Port:</strong> <span id="serial-port">Loading...</span></p>
            </div>
            
            <div class="info-box">
                <h3>Position Data</h3>
                <p><strong>Current Pitch:</strong> <span id="current-pitch" class="value">--</span>°</p>
                <p><strong>Current Roll:</strong> <span id="current-roll" class="value">--</span>°</p>
                <p><strong>Park Pitch:</strong> <span id="park-pitch" class="value">--</span>°</p>
                <p><strong>Park Roll:</strong> <span id="park-roll" class="value">--</span>°</p>
                <p><strong>Tolerance:</strong> <span id="tolerance" class="value">--</span>°</p>
            </div>
        </div>
        
        <div class="endpoints">
            <h3>ASCOM Alpaca Endpoints</h3>
            <div class="endpoint">GET /api/v1/safetymonitor/0/connected</div>
            <div class="endpoint">GET /api/v1/safetymonitor/0/issafe</div>
            <div class="endpoint">GET /api/v1/safetymonitor/0/name</div>
            <div class="endpoint">GET /api/v1/safetymonitor/0/description</div>
        </div>
        
        <div>
            <button onclick="refreshStatus()">🔄 Refresh Status</button>
            <button onclick="testConnection()">🧪 Test ASCOM</button>
            <button onclick="clearLog()">🗑️ Clear Log</button>
        </div>
        
        <h3>Activity Log</h3>
        <div id="log"></div>
    </div>

    <script>
        let logElement = document.getElementById('log');
        let currentlyConnected = false;
        
        function log(message) {
            const timestamp = new Date().toLocaleTimeString();
            logElement.textContent += '[' + timestamp + '] ' + message + '\n';
            logElement.scrollTop = logElement.scrollHeight;
        }
        
        function clearLog() {
            logElement.innerHTML = '';
        }
        
        async function fetchStatus() {
            try {
                const response = await fetch('/api/status');
                const data = await response.json();
                updateUI(data);
            } catch (error) {
                log('❌ Failed to fetch status: ' + error.message);
            }
        }
        
        async function refreshPorts() {
            try {
                const response = await fetch('/api/ports');
                const data = await response.json();
                const select = document.getElementById('port-select');
                
                select.innerHTML = '<option value="">Select a port...</option>';
                
                data.ports.forEach(port => {
                    const option = document.createElement('option');
                    option.value = port.name;
                    option.textContent = port.name + ' - ' + port.description;
                    select.appendChild(option);
                });
                
                log('🔄 Refreshed ' + data.ports.length + ' available ports');
            } catch (error) {
                log('❌ Failed to refresh ports: ' + error.message);
            }
        }
        
        async function connectToPort() {
            const port = document.getElementById('port-select').value;
            const baudRate = parseInt(document.getElementById('baud-rate').value);
            
            if (!port) {
                log('❌ Please select a port first');
                return;
            }
            
            try {
                const response = await fetch('/api/connect', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ port: port, baud_rate: baudRate })
                });
                
                const data = await response.json();
                
                if (data.success) {
                    log('✅ ' + data.message);
                    updateConnectionButtons(true);
                } else {
                    log('❌ Connection failed: ' + data.message);
                }
            } catch (error) {
                log('❌ Failed to connect: ' + error.message);
            }
        }
        
        async function disconnectFromPort() {
            try {
                const response = await fetch('/api/disconnect', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' }
                });
                
                const data = await response.json();
                
                if (data.success) {
                    log('✅ ' + data.message);
                    updateConnectionButtons(false);
                } else {
                    log('❌ Disconnect failed: ' + data.message);
                }
            } catch (error) {
                log('❌ Failed to disconnect: ' + error.message);
            }
        }
        
        function updateConnectionButtons(connected) {
            const connectBtn = document.getElementById('connect-btn');
            const disconnectBtn = document.getElementById('disconnect-btn');
            const portSelect = document.getElementById('port-select');
            const baudRate = document.getElementById('baud-rate');
            
            connectBtn.disabled = connected;
            disconnectBtn.disabled = !connected;
            portSelect.disabled = connected;
            baudRate.disabled = connected;
            
            currentlyConnected = connected;
        }
        
        function updateUI(data) {
            const connStatus = document.getElementById('connection-status');
            if (data.connected) {
                connStatus.className = 'status connected';
                connStatus.innerHTML = '✅ Connected to device';
                updateConnectionButtons(true);
            } else {
                connStatus.className = 'status disconnected';
                connStatus.innerHTML = '❌ Not connected to device';
                if (data.error_message) {
                    connStatus.innerHTML += ' - ' + data.error_message;
                }
                updateConnectionButtons(false);
            }
            
            const safetyStatus = document.getElementById('safety-status');
            if (data.connected) {
                if (data.is_safe) {
                    safetyStatus.className = 'status safe';
                    safetyStatus.innerHTML = '✅ Telescope is PARKED (Safe)';
                } else {
                    safetyStatus.className = 'status unsafe';
                    safetyStatus.innerHTML = '⚠️ Telescope is NOT PARKED (Unsafe)';
                }
            } else {
                safetyStatus.className = 'status unsafe';
                safetyStatus.innerHTML = '🚫 Safety status unknown (disconnected)';
            }
            
            document.getElementById('device-name').textContent = data.device_name;
            document.getElementById('device-version').textContent = data.device_version;
            document.getElementById('manufacturer').textContent = data.manufacturer;
            document.getElementById('serial-port').textContent = data.serial_port || 'Not connected';
            
            document.getElementById('current-pitch').textContent = data.current_pitch.toFixed(2);
            document.getElementById('current-roll').textContent = data.current_roll.toFixed(2);
            document.getElementById('park-pitch').textContent = data.park_pitch.toFixed(2);
            document.getElementById('park-roll').textContent = data.park_roll.toFixed(2);
            document.getElementById('tolerance').textContent = data.position_tolerance.toFixed(1);
        }
        
        function refreshStatus() {
            log('🔄 Refreshing status...');
            fetchStatus();
        }
        
        async function testConnection() {
            log('🧪 Testing ASCOM connection...');
            try {
                const response = await fetch('/api/v1/safetymonitor/0/connected');
                const data = await response.json();
                if (data.ErrorNumber === 0) {
                    log('✅ ASCOM test successful - Connected: ' + data.Value);
                } else {
                    log('❌ ASCOM test failed - Error: ' + data.ErrorMessage);
                }
            } catch (error) {
                log('❌ ASCOM test failed: ' + error.message);
            }
        }
        
        // Auto-refresh every 5 seconds
        setInterval(fetchStatus, 5000);
        
        // Initial load
        log('🚀 Web interface loaded');
        fetchStatus();
        refreshPorts();
    </script>
</body>
</html>"#)
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