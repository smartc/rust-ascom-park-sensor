use crate::device_state::{DeviceState, FirmwareResponse, StatusResponse, PositionResponse, ParkStatusResponse};
use crate::errors::{BridgeError, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tokio::time::{interval, timeout};
use tokio_serial::SerialPortBuilderExt;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

pub async fn run_serial_client(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
) -> Result<()> {
    // Create a cancellation token that never gets cancelled for main.rs usage
    let cancel_token = CancellationToken::new();
    run_serial_client_with_cancellation(port_name, baud_rate, device_state, cancel_token).await
}

pub async fn run_serial_client_with_cancellation(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
    cancel_token: CancellationToken,
) -> Result<()> {
    info!("Starting serial client for nRF52840 device on port: {}", port_name);

    {
        let mut state = device_state.write().await;
        state.serial_port = Some(port_name.clone());
        state.connected = false;
    }

    let result = connect_and_monitor_with_cancellation(&port_name, baud_rate, device_state.clone(), cancel_token).await;
    
    {
        let mut state = device_state.write().await;
        state.connected = false;
        state.serial_port = None;
        state.clear_error();
    }
    
    info!("Serial client stopped for port: {}", port_name);
    result
}

async fn connect_and_monitor_with_cancellation(
    port_name: &str,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
    cancel_token: CancellationToken,
) -> Result<()> {
    info!("Connecting to nRF52840 at {} at {} baud", port_name, baud_rate);
    
    let mut port = tokio_serial::new(port_name, baud_rate)
        .timeout(Duration::from_millis(1000))
        .data_bits(tokio_serial::DataBits::Eight)
        .flow_control(tokio_serial::FlowControl::None)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .open_native_async()
        .map_err(|e| {
            error!("Failed to open serial port {}: {}", port_name, e);
            BridgeError::Serial(e)
        })?;
    
    #[cfg(windows)]
    {
        use tokio_serial::SerialPort;
        if let Err(e) = port.write_data_terminal_ready(true) {
            warn!("Failed to set DTR: {}", e);
        } else {
            debug!("DTR set to true");
        }
        if let Err(e) = port.write_request_to_send(false) {
            warn!("Failed to set RTS: {}", e);
        } else {
            debug!("RTS set to false");
        }
    }
    
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let (reader, mut writer) = tokio::io::split(port);
    let mut reader = BufReader::new(reader);
    
    info!("Serial connection established to nRF52840 device");
    
    // Read startup messages with cancellation support
    info!("Reading device startup messages...");
    let start_time = std::time::Instant::now();
    let mut line_buffer = String::new();
    while start_time.elapsed() < Duration::from_secs(3) {
        line_buffer.clear();
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Cancelled during startup message reading");
                return Ok(());
            }
            result = tokio::time::timeout(Duration::from_millis(100), reader.read_line(&mut line_buffer)) => {
                match result {
                    Ok(Ok(bytes_read)) => {
                        if bytes_read > 0 {
                            debug!("Device startup message received");
                            if bytes_read > 10 { // Likely a real message
                                break;
                            }
                        }
                    }
                    _ => continue,
                }
            }
        }
    }
    
    {
        let mut state = device_state.write().await;
        state.connected = true;
        state.clear_error();
    }
    
    let mut status_interval = interval(Duration::from_secs(10));
    let mut position_interval = interval(Duration::from_secs(3));
    
    info!("Sending initial status query to nRF52840");
    if let Err(e) = send_command(&mut writer, "01").await {
        warn!("Failed to send initial status command: {}", e);
    }
    
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Serial client cancelled - exiting cleanly");
                break;
            }
            
            result = read_response(&mut reader) => {
                match result {
                    Ok(response) => {
                        if let Err(e) = process_response(response, device_state.clone()).await {
                            warn!("Error processing response: {}", e);
                        }
                    }
                    Err(BridgeError::Timeout) => {
                        debug!("No response from device (timeout)");
                    }
                    Err(e) => {
                        error!("Error reading from serial: {}", e);
                        break;
                    }
                }
            }
            
            _ = status_interval.tick() => {
                debug!("Polling device status");
                if let Err(e) = send_command(&mut writer, "01").await {
                    error!("Error sending status check: {}", e);
                    break;
                }
            }
            
            _ = position_interval.tick() => {
                debug!("Polling park status");
                if let Err(e) = send_command(&mut writer, "03").await {
                    error!("Error sending park status check: {}", e);
                    break;
                }
            }
        }
    }
    
    info!("Starting serial port cleanup for {}", port_name);
    drop(reader);
    drop(writer);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    {
        let mut state = device_state.write().await;
        state.connected = false;
        state.clear_error();
    }
    
    info!("Serial port {} released and connection monitor stopped", port_name);
    Ok(())
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
    if response.is_empty() || response.starts_with("=====") || response.starts_with("Device ready") {
        return Ok(());
    }
    
    if response.starts_with("=== ") || response.contains("Debug") {
        debug!("Device debug message: {}", response);
        return Ok(());
    }
    
    let parsed: FirmwareResponse = match serde_json::from_str(&response) {
        Ok(parsed) => parsed,
        Err(e) => {
            debug!("Non-JSON response from device: {} (parse error: {})", response, e);
            return Ok(());
        }
    };
    
    debug!("Parsed firmware response: status={}, has_data={}", 
           parsed.status, parsed.data.is_some());
    
    match parsed.status.as_str() {
        "ok" => {
            if let Some(data) = parsed.data {
                update_device_state_from_data(data, device_state).await?;
            }
        }
        "ack" => {
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
    
    if let Ok(status_data) = serde_json::from_value::<StatusResponse>(data.clone()) {
        debug!("Updating device status from nRF52840: parked={}, calibrated={}", 
               status_data.parked, status_data.calibrated);
        state.update_from_status(&status_data);
        return Ok(());
    }
    
    if let Ok(position_data) = serde_json::from_value::<PositionResponse>(data.clone()) {
        debug!("Updating position from nRF52840: pitch={:.2}, roll={:.2}", 
               position_data.pitch, position_data.roll);
        state.update_from_position(&position_data);
        return Ok(());
    }
    
    if let Ok(park_data) = serde_json::from_value::<ParkStatusResponse>(data.clone()) {
        debug!("Updating park status from nRF52840: parked={}, pitch={:.2}, roll={:.2}", 
               park_data.parked, park_data.current_pitch, park_data.current_roll);
        state.update_from_park_status(&park_data);
        return Ok(());
    }
    
    if let Some(message) = data.get("message") {
        if let Some(msg_str) = message.as_str() {
            info!("nRF52840 message: {}", msg_str);
            return Ok(());
        }
    }
    
    debug!("Unknown data format from nRF52840: {}", data);
    Ok(())
}