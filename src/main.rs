use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

mod serial_client;
mod alpaca_server;
mod device_state;
mod errors;
mod port_discovery;
mod telescope_client;

use crate::device_state::DeviceState;
use crate::alpaca_server::create_alpaca_server;
use crate::port_discovery::discover_ports;

#[derive(Parser, Debug)]
#[command(name = "telescope_park_bridge")]
#[command(about = "ASCOM Alpaca bridge for ESP32 Telescope Park Sensor")]
struct Args {
    /// Serial port (e.g., COM3, /dev/ttyUSB0)
    #[arg(short, long)]
    port: Option<String>,
    
    /// Baud rate for serial communication
    #[arg(short, long, default_value = "115200")]
    baud: u32,
    
    /// HTTP server bind address
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    
    /// HTTP server port for ASCOM Alpaca
    #[arg(long, default_value = "11111")]
    http_port: u16,
    
    /// Auto-select first available ESP32-like device
    #[arg(long)]
    auto: bool,
    
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize logging
    let log_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("telescope_park_bridge={}", log_level))
        .init();
    
    info!("Starting Telescope Park Bridge v{}", env!("CARGO_PKG_VERSION"));
    
    // Only use command-line specified port or auto-select, skip interactive selection
    let serial_port = if let Some(port) = args.port {
        info!("Using specified port: {}", port);
        Some(port)
    } else if args.auto {
        info!("Auto-selecting ESP32 device...");
        match discover_ports() {
            Ok(ports) => {
                // Look for ESP32-like devices first
                let mut found_port = None;
                for port in &ports {
                    if port.description.to_lowercase().contains("esp32") || 
                       port.description.to_lowercase().contains("ch340") ||
                       port.description.to_lowercase().contains("cp210") {
                        info!("Auto-selected ESP32-like device: {} ({})", port.name, port.description);
                        found_port = Some(port.name.clone());
                        break;
                    }
                }
                
                // If no ESP32-like device found, use first available
                found_port.or_else(|| {
                    if !ports.is_empty() {
                        info!("Auto-selected first available port: {}", ports[0].name);
                        Some(ports[0].name.clone())
                    } else {
                        None
                    }
                })
            }
            Err(_) => None,
        }
    } else {
        info!("Starting in web-only mode - use web interface to select serial port");
        None
    };
    
    if let Some(port) = &serial_port {
        info!("Using serial port: {}", port);
    } else {
        info!("No serial port selected - running in web-only mode");
    }
    
    // Create shared device state
    let device_state = Arc::new(RwLock::new(DeviceState::new()));
    
    // Start the ASCOM Alpaca server
    let server_handle = tokio::spawn(create_alpaca_server(
        args.bind.clone(),
        args.http_port,
        device_state.clone(),
    ));
    
    // Start serial communication if port was selected
    let serial_handle = if let Some(port) = serial_port {
        Some(tokio::spawn(serial_client::run_serial_client(
            port,
            args.baud,
            device_state.clone(),
        )))
    } else {
        None
    };
    
    info!("Bridge running at http://{}:{}", args.bind, args.http_port);
    info!("Web interface: http://{}:{}/", args.bind, args.http_port);
    info!("ASCOM Alpaca endpoint: http://{}:{}/api/v1/safetymonitor/0/", args.bind, args.http_port);
    
    if serial_handle.is_none() {
        info!("Running in web-only mode - use web interface to connect to serial device");
    }
    
    info!("Press Ctrl+C to stop");
    
    // Wait for either task to complete (or fail)
    match serial_handle {
        Some(handle) => {
            tokio::select! {
                result = server_handle => {
                    if let Err(e) = result {
                        error!("Server error: {}", e);
                    }
                }
                result = handle => {
                    if let Err(e) = result {
                        error!("Serial client error: {}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received Ctrl+C, shutting down...");
                }
            }
        }
        None => {
            tokio::select! {
                result = server_handle => {
                    if let Err(e) = result {
                        error!("Server error: {}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received Ctrl+C, shutting down...");
                }
            }
        }
    }
    
    Ok(())
}