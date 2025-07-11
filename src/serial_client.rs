use crate::device_state::DeviceState;
use crate::errors::{BridgeError, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tokio::time::{interval, timeout, Instant};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, error, info, warn};

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
    #[serde(rename = "buttonPressed")]
    button_pressed: bool,
    #[serde(rename = "ledStatus")]
    led_status: bool,
    #[serde(rename = "freeHeap")]
    free_heap: u64,
    uptime: u64,
}

pub async fn run_serial_client(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
) -> Result<()> {
    info!("Starting serial client for port: {}", port_name);

    // Update device state with port info
    {
        let mut state = device_state.write().await;
        state.serial_port = Some(port_name.clone());
    }

    loop {
        match connect_and_monitor(&port_name, baud_rate, device_state.clone()).await {
            Ok(()) => {
                info!("Serial connection ended normally");
                break;
            }
            Err(e) => {
                error!("Serial connection error: {}", e);
                
                // Update device state with error
                {
                    let mut state = device_state.write().await;
                    state.set_error(&format!("Serial error: {}", e));
                }
                
                // Wait before retrying
                warn!("Retrying connection in 5 seconds...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
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
    
    // Open serial port
    let port = tokio_serial::new(port_name, baud_rate)
        .timeout(Duration::from_millis(1000))
        .open_native_async()?;
    
    // Set up buffered reader
    let (reader, mut writer) = tokio::io::split(port);
    let mut reader = BufReader::new(reader);
    
    info!("Serial connection established");
    
    // Mark as connected and clear any errors
    {
        let mut state = device_state.write().await;
        state.connected = true;
        state.clear_error();
    }
    
    // Set up periodic polling
    let mut poll_interval = interval(Duration::from_secs(2));
    let mut status_interval = interval(Duration::from_secs(10));
    
    // Initial status query
    send_command(&mut writer, "01").await?;  // Get status
    
    loop {
        tokio::select! {
            // Handle incoming data
            result = read_response(&mut reader) => {
                match result {
                    Ok(response) => {
                        if let Err(e) = process_response(response, device_state.clone()).await {
                            error!("Error processing response: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error reading from serial: {}", e);
                        return Err(e);
                    }
                }
            }
            
            // Periodic position polling
            _ = poll_interval.tick() => {
                debug!("Polling device position");
                if let Err(e) = send_command(&mut writer, "03").await {  // Is parked?
                    error!("Error sending position poll: {}", e);
                    return Err(e);
                }
            }
            
            // Periodic status check
            _ = status_interval.tick() => {
                debug!("Checking device status");
                if let Err(e) = send_command(&mut writer, "01").await {  // Get status
                    error!("Error sending status check: {}", e);
                    return Err(e);
                }
            }
        }
    }
}

async fn send_command(writer: &mut tokio::io::WriteHalf<tokio_serial::SerialStream>, command: &str) -> Result<()> {
    let command_str = format!("<{}>\n", command);
    debug!("Sending command: {}", command_str.trim());
    
    writer.write_all(command_str.as_bytes()).await?;
    writer.flush().await?;
    
    Ok(())
}

async fn read_response(reader: &mut BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>) -> Result<String> {
    let mut line = String::new();
    
    // Add timeout to prevent hanging
    match timeout(Duration::from_secs(5), reader.read_line(&mut line)).await {
        Ok(Ok(_)) => {
            let trimmed = line.trim();
            debug!("Received: {}", trimmed);
            Ok(trimmed.to_string())
        }
        Ok(Err(e)) => Err(BridgeError::Io(e)),
        Err(_) => Err(BridgeError::Timeout),
    }
}

async fn process_response(response: String, device_state: Arc<RwLock<DeviceState>>) -> Result<()> {
    // Skip empty lines and notifications
    if response.is_empty() {
        return Ok(());
    }
    
    // Try to parse as JSON
    let parsed: SerialResponse = match serde_json::from_str(&response) {
        Ok(parsed) => parsed,
        Err(_) => {
            debug!("Non-JSON response (possibly notification): {}", response);
            return Ok(());  // Not an error, just ignore non-JSON responses
        }
    };
    
    debug!("Parsed response: {:?}", parsed);
    
    // Handle different response types
    match parsed.status.as_str() {
        "ok" => {
            if let Some(data) = parsed.data {
                update_device_state_from_data(data, device_state).await?;
            }
        }
        "ack" => {
            // Command acknowledged, just log it
            if let Some(command) = parsed.command {
                debug!("Command {} acknowledged", command);
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
    
    // Try to parse as different data types
    
    // Check if it's position data
    if let Ok(position_data) = serde_json::from_value::<PositionData>(data.clone()) {
        debug!("Updating position: pitch={}, roll={}", position_data.pitch, position_data.roll);
        state.current_pitch = position_data.pitch;
        state.current_roll = position_data.roll;
        state.update_timestamp();
        return Ok(());
    }
    
    // Check if it's parked status data
    if let Ok(parked_data) = serde_json::from_value::<ParkedData>(data.clone()) {
        debug!("Updating park status: parked={}", parked_data.parked);
        state.is_safe = parked_data.parked;  // In ASCOM, "safe" means parked
        state.current_pitch = parked_data.current_pitch;
        state.current_roll = parked_data.current_roll;
        state.park_pitch = parked_data.park_pitch;
        state.park_roll = parked_data.park_roll;
        state.position_tolerance = parked_data.tolerance;
        state.update_timestamp();
        return Ok(());
    }
    
    // Check if it's status data
    if let Ok(status_data) = serde_json::from_value::<StatusData>(data.clone()) {
        debug!("Updating device status");
        state.device_name = status_data.device_name;
        state.device_version = status_data.version;
        state.manufacturer = status_data.manufacturer;
        state.is_safe = status_data.parked;
        state.update_timestamp();
        return Ok(());
    }
    
    // If we can't parse it as any known type, just log it
    debug!("Unknown data format: {}", data);
    Ok(())
}