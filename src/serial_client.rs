use crate::device_state::{DeviceState, FirmwareResponse, StatusResponse, PositionResponse, ParkStatusResponse};
use crate::errors::{BridgeError, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tokio::time::{interval, timeout};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, error, info, warn};

pub async fn run_serial_client(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
) -> Result<()> {
    info!("Starting serial client for nRF52840 device on port: {}", port_name);

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
    info!("Connecting to nRF52840 at {} at {} baud", port_name, baud_rate);
    
    // Open serial port with settings appropriate for nRF52840
    let port = tokio_serial::new(port_name, baud_rate)
        .timeout(Duration::from_millis(1000))
        .data_bits(tokio_serial::DataBits::Eight)
        .flow_control(tokio_serial::FlowControl::None)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .open_native_async()?;
    
    // Set up buffered reader/writer
    let (reader, mut writer) = tokio::io::split(port);
    let mut reader = BufReader::new(reader);
    
    info!("Serial connection established to nRF52840 device");
    
    // Mark as connected and clear any errors
    {
        let mut state = device_state.write().await;
        state.connected = true;
        state.clear_error();
    }
    
    // Wait a moment for device to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Set up periodic polling intervals
    let mut status_interval = interval(Duration::from_secs(10)); // Device status every 10s
    let mut position_interval = interval(Duration::from_secs(3)); // Position/park status every 3s
    
    // Initial device inquiry - wait a bit more for device to be ready
    tokio::time::sleep(Duration::from_millis(1000)).await;
    info!("Sending initial status query to nRF52840");
    send_command(&mut writer, "01").await?;  // Get device status
    
    loop {
        tokio::select! {
            // Handle incoming data
            result = read_response(&mut reader) => {
                match result {
                    Ok(response) => {
                        if let Err(e) = process_response(response, device_state.clone()).await {
                            warn!("Error processing response: {}", e);
                        }
                    }
                    Err(BridgeError::Timeout) => {
                        // Timeout is not necessarily an error, just continue
                        debug!("No response from device (timeout)");
                    }
                    Err(e) => {
                        error!("Error reading from serial: {}", e);
                        return Err(e);
                    }
                }
            }
            
            // Periodic device status check
            _ = status_interval.tick() => {
                debug!("Polling device status");
                if let Err(e) = send_command(&mut writer, "01").await {  // CMD_GET_STATUS
                    error!("Error sending status check: {}", e);
                    return Err(e);
                }
            }
            
            // Periodic position and park status check
            _ = position_interval.tick() => {
                debug!("Polling park status");
                if let Err(e) = send_command(&mut writer, "03").await {  // CMD_IS_PARKED
                    error!("Error sending park status check: {}", e);
                    return Err(e);
                }
            }
        }
    }
}

async fn send_command(writer: &mut tokio::io::WriteHalf<tokio_serial::SerialStream>, command: &str) -> Result<()> {
    let command_str = format!("<{}>\n", command);
    debug!("Sending command to nRF52840: {}", command_str.trim());
    
    writer.write_all(command_str.as_bytes()).await?;
    writer.flush().await?;
    
    Ok(())
}

async fn read_response(reader: &mut BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>) -> Result<String> {
    let mut line = String::new();
    
    // Add timeout to prevent hanging - reduced to 3 seconds
    match timeout(Duration::from_secs(3), reader.read_line(&mut line)).await {
        Ok(Ok(bytes_read)) => {
            if bytes_read == 0 {
                return Err(BridgeError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Device disconnected"
                )));
            }
            
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                debug!("Received from nRF52840: {}", trimmed);
            }
            Ok(trimmed.to_string())
        }
        Ok(Err(e)) => {
            error!("IO error reading from nRF52840: {}", e);
            Err(BridgeError::Io(e))
        }
        Err(_) => {
            debug!("Timeout waiting for nRF52840 response");
            Err(BridgeError::Timeout)
        }
    }
}

async fn process_response(response: String, device_state: Arc<RwLock<DeviceState>>) -> Result<()> {
    // Skip empty lines and device startup messages
    if response.is_empty() || response.starts_with("=====") || response.starts_with("Device ready") {
        return Ok(());
    }
    
    // Skip debug messages from device
    if response.starts_with("=== ") || response.contains("Debug") {
        debug!("Device debug message: {}", response);
        return Ok(());
    }
    
    // Try to parse as JSON
    let parsed: FirmwareResponse = match serde_json::from_str(&response) {
        Ok(parsed) => parsed,
        Err(e) => {
            // Log non-JSON responses as device messages
            debug!("Non-JSON response from device: {} (parse error: {})", response, e);
            return Ok(());
        }
    };
    
    debug!("Parsed firmware response: status={}, has_data={}", 
           parsed.status, parsed.data.is_some());
    
    // Handle different response types based on firmware's serial_interface.cpp
    match parsed.status.as_str() {
        "ok" => {
            if let Some(data) = parsed.data {
                update_device_state_from_data(data, device_state).await?;
            }
        }
        "ack" => {
            // Command acknowledged, just log it
            if let Some(command) = parsed.command {
                debug!("Command {} acknowledged by nRF52840", command);
            }
        }
        "error" => {
            let error_msg = parsed.message.unwrap_or_else(|| "Unknown device error".to_string());
            warn!("nRF52840 reported error: {}", error_msg);
            
            let mut state = device_state.write().await;
            state.set_error(&error_msg);
        }
        _ => {
            warn!("Unknown response status from nRF52840: {}", parsed.status);
        }
    }
    
    Ok(())
}

async fn update_device_state_from_data(
    data: serde_json::Value,
    device_state: Arc<RwLock<DeviceState>>,
) -> Result<()> {
    let mut state = device_state.write().await;
    
    // Try to parse as different data types based on the firmware responses
    
    // Check if it's device status data (from CMD_GET_STATUS - "01")
    if let Ok(status_data) = serde_json::from_value::<StatusResponse>(data.clone()) {
        debug!("Updating device status from nRF52840: parked={}, calibrated={}", 
               status_data.parked, status_data.calibrated);
        state.update_from_status(&status_data);
        return Ok(());
    }
    
    // Check if it's position data (from CMD_GET_POSITION - "02")
    if let Ok(position_data) = serde_json::from_value::<PositionResponse>(data.clone()) {
        debug!("Updating position from nRF52840: pitch={:.2}, roll={:.2}", 
               position_data.pitch, position_data.roll);
        state.update_from_position(&position_data);
        return Ok(());
    }
    
    // Check if it's park status data (from CMD_IS_PARKED - "03")
    if let Ok(park_data) = serde_json::from_value::<ParkStatusResponse>(data.clone()) {
        debug!("Updating park status from nRF52840: parked={}, pitch={:.2}, roll={:.2}", 
               park_data.parked, park_data.current_pitch, park_data.current_roll);
        state.update_from_park_status(&park_data);
        return Ok(());
    }
    
    // If it's a simple message response
    if let Some(message) = data.get("message") {
        if let Some(msg_str) = message.as_str() {
            info!("nRF52840 message: {}", msg_str);
            return Ok(());
        }
    }
    
    // Log unknown data format for debugging
    debug!("Unknown data format from nRF52840: {}", data);
    Ok(())
}

// Public function to send commands from web interface
pub async fn send_device_command(
    device_state: Arc<RwLock<DeviceState>>,
    command: &str,
) -> Result<String> {
    // This function would need access to the writer, which we'd need to refactor
    // For now, return an error indicating this needs implementation
    Err(BridgeError::CommandFailed("Command sending not yet implemented".to_string()))
}