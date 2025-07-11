use crate::device_state::DeviceState;
use crate::errors::{BridgeError, Result};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tokio::time::{interval, timeout};
use tokio_serial::SerialPort;
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, error, info, warn};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
struct SerialResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PositionData {
    pitch: f32,
    roll: f32,
}

#[derive(Debug, Deserialize)]
struct ParkedData {
    parked: bool,
    #[serde(rename = "currentPitch")]
    current_pitch: f32,
    #[serde(rename = "currentRoll")]
    current_roll: f32,
    #[serde(rename = "parkPitch")]
    park_pitch: f32,
    #[serde(rename = "parkRoll")]
    park_roll: f32,
    tolerance: f32,
    #[serde(rename = "pitchDiff")]
    pitch_diff: f32,
    #[serde(rename = "rollDiff")]
    roll_diff: f32,
}

#[derive(Debug, Deserialize)]
struct StatusData {
    #[serde(rename = "deviceName")]
    device_name: String,
    version: String,
    manufacturer: String,
    parked: bool,
    calibrated: bool,
    #[serde(rename = "ledStatus")]
    led_status: bool,
    uptime: u64,
}

pub async fn run_serial_client(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    info!("Starting serial client for port: {}", port_name);

    // Update device state with port info
    {
        let mut state = device_state.write().await;
        state.serial_port = Some(port_name.clone());
        state.connected = false;
    }

    tokio::select! {
        result = connect_and_monitor(&port_name, baud_rate, device_state.clone()) => {
            match result {
                Ok(()) => info!("Serial connection ended normally"),
                Err(e) => {
                    error!("Serial connection error: {}", e);
                    let mut state = device_state.write().await;
                    state.set_error(&format!("Serial error: {}", e));
                }
            }
        }
        _ = cancellation_token.cancelled() => {
            info!("Serial client cancelled");
            let mut state = device_state.write().await;
            state.connected = false;
            state.serial_port = None;
            state.clear_error();
        }
    }
    
    Ok(())
}

async fn connect_and_monitor(
    port_name: &str,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
) -> Result<()> {
    info!("Connecting to {} at {} baud", port_name, baud_rate);
    
    // Add a small delay to ensure port is available
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Open serial port with explicit settings for nRF52840
    let mut port_builder = tokio_serial::new(port_name, baud_rate)
        .timeout(Duration::from_millis(1000));
    
    // Set explicit serial parameters for nRF52840
    port_builder = port_builder
        .data_bits(tokio_serial::DataBits::Eight)
        .flow_control(tokio_serial::FlowControl::None)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One);
    
    let mut port = port_builder.open_native_async()?;
    
    // CRITICAL: Set DTR and RTS for nRF52840 communication
    if let Err(e) = port.write_data_terminal_ready(true) {
        warn!("Failed to set DTR: {}", e);
    }
    if let Err(e) = port.write_request_to_send(true) {
        warn!("Failed to set RTS: {}", e);
    }
    
    info!("DTR and RTS set for nRF52840 communication");
    
    // Give the device a moment to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Set up buffered reader/writer
    let (reader, mut writer) = tokio::io::split(port);
    let mut reader = BufReader::new(reader);
    
    info!("Serial connection established");
    
    // Mark as connected and clear any errors
    {
        let mut state = device_state.write().await;
        state.connected = true;
        state.clear_error();
    }
    
    // Set up periodic polling with longer intervals to avoid overwhelming device
    let mut poll_interval = interval(Duration::from_secs(5));
    let mut status_interval = interval(Duration::from_secs(15));
    
    // Wait a moment for device to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Initial status query
    info!("Sending initial status query");
    send_command(&mut writer, "01").await?;
    
    loop {
        tokio::select! {
            // Handle incoming data
            result = read_response(&mut reader) => {
                match result {
                    Ok(response) => {
                        if !response.is_empty() {
                            debug!("Processing response: {}", response);
                            if let Err(e) = process_response(response, device_state.clone()).await {
                                warn!("Error processing response: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading from serial: {}", e);
                        return Err(e);
                    }
                }
            }
            
            // Periodic park status polling
            _ = poll_interval.tick() => {
                debug!("Polling device park status");
                if let Err(e) = send_command(&mut writer, "03").await {
                    error!("Error sending park status poll: {}", e);
                    return Err(e);
                }
            }
            
            // Periodic general status check
            _ = status_interval.tick() => {
                debug!("Checking device general status");
                if let Err(e) = send_command(&mut writer, "01").await {
                    error!("Error sending status check: {}", e);
                    return Err(e);
                }
            }
        }
    }
}

async fn send_command(writer: &mut tokio::io::WriteHalf<tokio_serial::SerialStream>, command: &str) -> Result<()> {
    let command_str = format!("<{}>\r\n", command);
    info!("Sending command: {}", command_str.trim());
    
    writer.write_all(command_str.as_bytes()).await?;
    writer.flush().await?;
    
    // Add a small delay after sending command
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    Ok(())
}

async fn read_response(reader: &mut BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>) -> Result<String> {
    let mut line = String::new();
    
    // Use longer timeout since nRF52840 might be slower to respond
    match timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(bytes_read)) => {
            if bytes_read == 0 {
                debug!("EOF reached on serial connection");
                return Err(BridgeError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Serial connection closed"
                )));
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                info!("Received from device: {}", trimmed);
            }
            Ok(trimmed.to_string())
        }
        Ok(Err(e)) => Err(BridgeError::Io(e)),
        Err(_) => {
            debug!("Timeout waiting for device response");
            Ok(String::new()) // Return empty string on timeout instead of error
        }
    }
}

async fn process_response(response: String, device_state: Arc<RwLock<DeviceState>>) -> Result<()> {
    // Skip empty lines and debug messages
    if response.is_empty() {
        return Ok(());
    }
    
    // Check for device startup messages
    if response.contains("Telescope Park Sensor") || 
       response.contains("nRF52840") || 
       response.contains("Device ready") ||
       response.starts_with("===") {
        info!("Device message: {}", response);
        return Ok(());
    }
    
    // Try to parse as JSON
    let parsed: SerialResponse = match serde_json::from_str(&response) {
        Ok(parsed) => {
            info!("Successfully parsed JSON response: {:?}", parsed);
            parsed
        },
        Err(e) => {
            warn!("Failed to parse as JSON: {} - Response was: {}", e, response);
            // If it's not JSON, treat as a raw message
            info!("Raw device message: {}", response);
            return Ok(());
        }
    };
    
    // Handle different response types
    match parsed.status.as_str() {
        "ok" => {
            info!("Received OK response");
            if let Some(data) = parsed.data {
                info!("Processing data: {:?}", data);
                update_device_state_from_data(data, device_state).await?;
            }
        }
        "ack" => {
            if let Some(command) = parsed.command {
                info!("Command {} acknowledged", command);
            }
        }
        "error" => {
            let error_msg = parsed.message.unwrap_or_else(|| "Unknown device error".to_string());
            warn!("Device reported error: {}", error_msg);
            
            let mut state = device_state.write().await;
            state.set_error(&error_msg);
        }
        _ => {
            warn!("Unknown response status: {}", parsed.status);
        }
    }
    
    Ok(())
}

async fn update_device_state_from_data(
    data: serde_json::Value,
    device_state: Arc<RwLock<DeviceState>>,
) -> Result<()> {
    let mut state = device_state.write().await;
    
    info!("Updating device state with data: {:?}", data);
    
    // Check if it's parked status data (highest priority)
    if let Ok(parked_data) = serde_json::from_value::<ParkedData>(data.clone()) {
        info!("Updating park status: parked={}", parked_data.parked);
        info!("  Current position: pitch={:.2}, roll={:.2}", parked_data.current_pitch, parked_data.current_roll);
        info!("  Park position: pitch={:.2}, roll={:.2}", parked_data.park_pitch, parked_data.park_roll);
        info!("  Tolerance: {:.1}, Pitch diff: {:.2}, Roll diff: {:.2}", 
               parked_data.tolerance, parked_data.pitch_diff, parked_data.roll_diff);
        
        state.is_safe = parked_data.parked;
        state.current_pitch = parked_data.current_pitch;
        state.current_roll = parked_data.current_roll;
        state.park_pitch = parked_data.park_pitch;
        state.park_roll = parked_data.park_roll;
        state.position_tolerance = parked_data.tolerance;
        state.update_timestamp();
        
        info!("✓ Park status updated: {} (pitch: {:.2}°, roll: {:.2}°)", 
              if parked_data.parked { "PARKED" } else { "NOT PARKED" },
              parked_data.current_pitch, parked_data.current_roll);
        
        return Ok(());
    }
    
    // Check if it's position data only
    if let Ok(position_data) = serde_json::from_value::<PositionData>(data.clone()) {
        info!("Updating position: pitch={}, roll={}", position_data.pitch, position_data.roll);
        state.current_pitch = position_data.pitch;
        state.current_roll = position_data.roll;
        state.update_timestamp();
        return Ok(());
    }
    
    // Check if it's general status data
    if let Ok(status_data) = serde_json::from_value::<StatusData>(data.clone()) {
        info!("Updating device status");
        state.device_name = status_data.device_name;
        state.device_version = status_data.version;
        state.manufacturer = status_data.manufacturer;
        state.is_safe = status_data.parked;
        state.update_timestamp();
        
        info!("✓ Device status updated: {} v{} by {}", 
              state.device_name, state.device_version, state.manufacturer);
        
        return Ok(());
    }
    
    // Try to extract individual fields if it's a mixed data object
    if let serde_json::Value::Object(obj) = &data {
        let mut updated = false;
        
        for (key, value) in obj {
            match key.as_str() {
                "parked" => {
                    if let Some(parked) = value.as_bool() {
                        state.is_safe = parked;
                        updated = true;
                        info!("Updated park status: {}", parked);
                    }
                }
                "currentPitch" => {
                    if let Some(pitch) = value.as_f64() {
                        state.current_pitch = pitch as f32;
                        updated = true;
                        info!("Updated current pitch: {}", pitch);
                    }
                }
                "currentRoll" => {
                    if let Some(roll) = value.as_f64() {
                        state.current_roll = roll as f32;
                        updated = true;
                        info!("Updated current roll: {}", roll);
                    }
                }
                "parkPitch" => {
                    if let Some(park_pitch) = value.as_f64() {
                        state.park_pitch = park_pitch as f32;
                        updated = true;
                        info!("Updated park pitch: {}", park_pitch);
                    }
                }
                "parkRoll" => {
                    if let Some(park_roll) = value.as_f64() {
                        state.park_roll = park_roll as f32;
                        updated = true;
                        info!("Updated park roll: {}", park_roll);
                    }
                }
                "tolerance" => {
                    if let Some(tolerance) = value.as_f64() {
                        state.position_tolerance = tolerance as f32;
                        updated = true;
                        info!("Updated tolerance: {}", tolerance);
                    }
                }
                "deviceName" => {
                    if let Some(device_name) = value.as_str() {
                        state.device_name = device_name.to_string();
                        updated = true;
                        info!("Updated device name: {}", device_name);
                    }
                }
                "version" => {
                    if let Some(version) = value.as_str() {
                        state.device_version = version.to_string();
                        updated = true;
                        info!("Updated version: {}", version);
                    }
                }
                "manufacturer" => {
                    if let Some(manufacturer) = value.as_str() {
                        state.manufacturer = manufacturer.to_string();
                        updated = true;
                        info!("Updated manufacturer: {}", manufacturer);
                    }
                }
                _ => {
                    debug!("Unhandled field: {} = {:?}", key, value);
                }
            }
        }
        
        if updated {
            state.update_timestamp();
            info!("✓ Updated device state from mixed data object");
        }
        
        return Ok(());
    }
    
    warn!("Unknown data format, could not parse: {}", data);
    Ok(())
}