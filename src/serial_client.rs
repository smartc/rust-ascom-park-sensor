// Serial client for nRF52840 device communication
// Updated v0.3.1 with command support
// Optimized for real-time updates with reduced debug noise:
// - Position polling: every 1 second 
// - Status polling: every 2 seconds
// - Debug messages: reduced frequency to minimize log noise
// - Park status changes: always logged as INFO level
// - NEW: Command sending support via mpsc channel

use crate::device_state::{DeviceState, FirmwareResponse, StatusResponse, PositionResponse, ParkStatusResponse};
use crate::errors::{BridgeError, Result};
use crate::connection_manager::CommandRequest;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{RwLock, mpsc};
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
    let (_cmd_sender, cmd_receiver) = mpsc::unbounded_channel::<CommandRequest>();
    run_serial_client_with_commands(port_name, baud_rate, device_state, cancel_token, cmd_receiver).await
}

pub async fn run_serial_client_with_cancellation(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
    cancel_token: CancellationToken,
) -> Result<()> {
    let (_cmd_sender, cmd_receiver) = mpsc::unbounded_channel::<CommandRequest>();
    run_serial_client_with_commands(port_name, baud_rate, device_state, cancel_token, cmd_receiver).await
}

pub async fn run_serial_client_with_commands(
    port_name: String,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
    cancel_token: CancellationToken,
    mut cmd_receiver: mpsc::UnboundedReceiver<CommandRequest>,
) -> Result<()> {
    info!("Starting serial client for nRF52840 device on port: {}", port_name);

    {
        let mut state = device_state.write().await;
        state.serial_port = Some(port_name.clone());
        state.connected = false;
    }

    let result = connect_and_monitor_with_commands(&port_name, baud_rate, device_state.clone(), cancel_token, &mut cmd_receiver).await;
    
    {
        let mut state = device_state.write().await;
        state.reset_to_disconnected();
    }
    
    info!("Serial client stopped for port: {}", port_name);
    result
}

async fn connect_and_monitor_with_commands(
    port_name: &str,
    baud_rate: u32,
    device_state: Arc<RwLock<DeviceState>>,
    cancel_token: CancellationToken,
    cmd_receiver: &mut mpsc::UnboundedReceiver<CommandRequest>,
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
    
    // FASTER POLLING INTERVALS for more real-time updates
    let mut status_interval = interval(Duration::from_secs(2));    // Device status every 2 seconds
    let mut position_interval = interval(Duration::from_secs(1));  // Park position every 1 second
    
    // Debug counters to reduce log noise
    let mut status_poll_count = 0u32;
    let mut position_poll_count = 0u32;
    
    info!("Sending initial status query to nRF52840");
    if let Err(e) = send_command(&mut writer, "01").await {
        warn!("Failed to send initial status command: {}", e);
    }
    
    // Pending command state for command-response matching
    let mut pending_commands: Vec<(String, tokio::sync::oneshot::Sender<Result<String>>)> = Vec::new();
    
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                info!("Serial client cancelled - exiting cleanly");
                break;
            }
            
            // Handle incoming commands
            cmd_request = cmd_receiver.recv() => {
                if let Some(cmd_req) = cmd_request {
                    info!("Processing command: {}", cmd_req.command);
                    
                    match send_command(&mut writer, &cmd_req.command).await {
                        Ok(()) => {
                            // Store the command for response matching
                            pending_commands.push((cmd_req.command.clone(), cmd_req.response_sender));
                            info!("Command {} sent, waiting for response", cmd_req.command);
                        }
                        Err(e) => {
                            error!("Failed to send command {}: {}", cmd_req.command, e);
                            let _ = cmd_req.response_sender.send(Err(e));
                        }
                    }
                } else {
                    // Command channel closed - this happens during shutdown
                    debug!("Command channel closed");
                }
            }
            
            result = read_response(&mut reader) => {
                match result {
                    Ok(response) => {
                        // Check if this response is for a pending command
                        let mut handled_as_command = false;
                        
                        // Try to match response to pending commands
                        if !pending_commands.is_empty() {
                            // For now, we'll just match the first pending command
                            // TODO: More sophisticated command-response matching based on response content
                            if let Some((command, sender)) = pending_commands.pop() {
                                info!("Command {} response: {}", command, response);
                                let _ = sender.send(Ok(response.clone()));
                                handled_as_command = true;
                            }
                        }
                        
                        // Always process response for state updates, even if it was for a command
                        if let Err(e) = process_response(response, device_state.clone()).await {
                            warn!("Error processing response: {}", e);
                        }
                    }
                    Err(BridgeError::Timeout) => {
                        // Only log timeouts occasionally to reduce noise
                        static mut TIMEOUT_COUNT: u32 = 0;
                        unsafe {
                            TIMEOUT_COUNT += 1;
                            if TIMEOUT_COUNT % 20 == 0 {
                                debug!("No response from device (timeout) - cycle {}", TIMEOUT_COUNT);
                            }
                        }
                        
                        // Check for timed out commands
                        if !pending_commands.is_empty() {
                            debug!("Timing out {} pending commands", pending_commands.len());
                            for (command, sender) in pending_commands.drain(..) {
                                warn!("Command {} timed out", command);
                                let _ = sender.send(Err(BridgeError::Timeout));
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading from serial: {}", e);
                        
                        // Fail any pending commands
                        for (command, sender) in pending_commands.drain(..) {
                            error!("Command {} failed due to serial error", command);
                            let _ = sender.send(Err(BridgeError::Device("Serial connection failed".to_string())));
                        }
                        break;
                    }
                }
            }
            
            _ = status_interval.tick() => {
                status_poll_count += 1;
                // Only log every 5th status poll (every 10 seconds) to reduce noise
                if status_poll_count % 5 == 0 {
                    debug!("Polling device status (cycle {})", status_poll_count);
                }
                if let Err(e) = send_command(&mut writer, "01").await {
                    error!("Error sending status check: {}", e);
                    break;
                }
            }
            
            _ = position_interval.tick() => {
                position_poll_count += 1;
                // Only log every 10th position poll (every 10 seconds) to reduce noise
                if position_poll_count % 10 == 0 {
                    debug!("Polling park status (cycle {})", position_poll_count);
                }
                if let Err(e) = send_command(&mut writer, "03").await {
                    error!("Error sending park status check: {}", e);
                    break;
                }
            }
        }
    }
    
    // Clean up any remaining pending commands
    for (command, sender) in pending_commands.drain(..) {
        warn!("Cleaning up pending command: {}", command);
        let _ = sender.send(Err(BridgeError::Device("Connection closed".to_string())));
    }
    
    info!("Starting serial port cleanup for {}", port_name);
    drop(reader);
    drop(writer);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    {
        let mut state = device_state.write().await;
        state.reset_to_disconnected();
    }
    
    info!("Serial port {} released and connection monitor stopped", port_name);
    Ok(())
}

async fn send_command(writer: &mut tokio::io::WriteHalf<tokio_serial::SerialStream>, command: &str) -> Result<()> {
    let command_str = format!("<{}>\n", command);
    
    // Log all command sends for debugging
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
                // Reduce debug noise for received messages - only log every 20th response
                static mut RECEIVE_COUNT: u32 = 0;
                unsafe {
                    RECEIVE_COUNT += 1;
                    if RECEIVE_COUNT % 20 == 0 {
                        debug!("Received from nRF52840: {} (cycle {})", trimmed, RECEIVE_COUNT);
                    }
                }
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
    
    // Reduce debug noise - only log parsed responses occasionally
    static mut RESPONSE_COUNT: u32 = 0;
    unsafe {
        RESPONSE_COUNT += 1;
        if RESPONSE_COUNT % 20 == 0 {
            debug!("Parsed firmware response: status={}, has_data={} (cycle {})", 
                   parsed.status, parsed.data.is_some(), RESPONSE_COUNT);
        }
    }
    
    match parsed.status.as_str() {
        "ok" => {
            if let Some(data) = parsed.data {
                update_device_state_from_data(data, device_state).await?;
            }
        }
        "ack" => {
            // Reduce ACK message noise - only log occasionally
            if let Some(command) = parsed.command {
                unsafe {
                    if RESPONSE_COUNT % 20 == 0 {
                        debug!("Command {} acknowledged by nRF52840 (recent acks logged)", command);
                    }
                }
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
    
    // Static counter to reduce debug noise
    static mut UPDATE_COUNT: u32 = 0;
    unsafe { UPDATE_COUNT += 1; }
    
    if let Ok(status_data) = serde_json::from_value::<StatusResponse>(data.clone()) {
        // Only log status updates every 10th time (every ~20 seconds)
        unsafe {
            if UPDATE_COUNT % 10 == 0 {
                debug!("Updating device status from nRF52840: parked={}, calibrated={} (cycle {})", 
                       status_data.parked, status_data.calibrated, UPDATE_COUNT);
            }
        }
        state.update_from_status(&status_data);
        return Ok(());
    }
    
    if let Ok(position_data) = serde_json::from_value::<PositionResponse>(data.clone()) {
        // Only log position updates every 20th time (every ~20 seconds)
        unsafe {
            if UPDATE_COUNT % 20 == 0 {
                debug!("Updating position from nRF52840: pitch={:.2}, roll={:.2} (cycle {})", 
                       position_data.pitch, position_data.roll, UPDATE_COUNT);
            }
        }
        state.update_from_position(&position_data);
        return Ok(());
    }
    
    if let Ok(park_data) = serde_json::from_value::<ParkStatusResponse>(data.clone()) {
        // Always log park status changes (important events), log periodic updates less frequently
        let was_parked = state.is_parked;
        let now_parked = park_data.parked;
        
        if was_parked != now_parked {
            info!("Park status CHANGED: {} -> {} at pitch={:.2}°, roll={:.2}°", 
                  if was_parked { "PARKED" } else { "NOT PARKED" },
                  if now_parked { "PARKED" } else { "NOT PARKED" },
                  park_data.current_pitch, park_data.current_roll);
        } else {
            unsafe {
                if UPDATE_COUNT % 20 == 0 {
                    debug!("Updating park status from nRF52840: parked={}, pitch={:.2}, roll={:.2} (cycle {})", 
                           park_data.parked, park_data.current_pitch, park_data.current_roll, UPDATE_COUNT);
                }
            }
        }
        
        state.update_from_park_status(&park_data);
        return Ok(());
    }
    
    if let Some(message) = data.get("message") {
        if let Some(msg_str) = message.as_str() {
            info!("nRF52840 message: {}", msg_str);
            return Ok(());
        }
    }
    
    // Only log unknown data occasionally
    unsafe {
        if UPDATE_COUNT % 50 == 0 {
            debug!("Unknown data format from nRF52840: {}", data);
        }
    }
    Ok(())
}