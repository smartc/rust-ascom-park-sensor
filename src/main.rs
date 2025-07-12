use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};

mod serial_client;
mod alpaca_server;
mod device_state;
mod errors;
mod port_discovery;
mod connection_manager;

use crate::device_state::DeviceState;
use crate::alpaca_server::create_alpaca_server;
use crate::port_discovery::discover_ports;
use crate::connection_manager::ConnectionManager;

#[derive(Parser, Debug)]
#[command(name = "telescope_park_bridge")]
#[command(about = "ASCOM Alpaca bridge for nRF52840 Telescope Park Sensor v0.3.1")]
#[command(version = "0.3.1")]
struct Args {
    /// Serial port (e.g., COM3, /dev/ttyUSB0, /dev/ttyACM0)
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
    
    /// Auto-select first available nRF52840-like device and connect
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
    
    info!("Starting Telescope Park Bridge v0.3.1");
    info!("New features: Device control commands (Set Park, Calibrate, Factory Reset, Manual Commands)");
    info!("Target device: nRF52840 XIAO Sense with built-in IMU");
    
    // Create shared device state
    let device_state = Arc::new(RwLock::new(DeviceState::new()));
    
    // Create connection manager
    let connection_manager = Arc::new(ConnectionManager::new(device_state.clone()));
    
    // Handle port selection and auto-connection
    let target_port = if let Some(port) = args.port {
        info!("Using specified port: {}", port);
        Some(port)
    } else if args.auto {
        info!("Auto-selecting nRF52840 device...");
        match discover_ports() {
            Ok(ports) => {
                // Look for nRF52840-like devices first
                let mut found_port = None;
                for port in &ports {
                    let desc_lower = port.description.to_lowercase();
                    if desc_lower.contains("nrf52") || 
                       desc_lower.contains("xiao") ||
                       desc_lower.contains("seeed") ||
                       desc_lower.contains("ch340") ||
                       desc_lower.contains("cp210") {
                        info!("Auto-selected nRF52840-like device: {} ({})", port.name, port.description);
                        found_port = Some(port.name.clone());
                        break;
                    }
                }
                
                // If no nRF52840-like device found, use first available
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
        None
    };
    
    // Auto-connect if we have a target port
    if let Some(port) = target_port {
        info!("Auto-connecting to: {}", port);
        match connection_manager.connect(port.clone(), args.baud).await {
            Ok(message) => info!("Auto-connect: {}", message),
            Err(e) => error!("Auto-connect failed: {}", e),
        }
    } else {
        info!("No port specified - use web interface to select and connect");
    }
    
    // Start the ASCOM Alpaca server with the connection manager
    let server_handle = tokio::spawn(create_alpaca_server(
        args.bind.clone(),
        args.http_port,
        device_state.clone(),
        connection_manager.clone(),
    ));
    
    info!("Bridge running at http://{}:{}", args.bind, args.http_port);
    info!("Web interface: http://{}:{}/", args.bind, args.http_port);
    info!("ASCOM Alpaca endpoint: http://{}:{}/api/v1/safetymonitor/0/", args.bind, args.http_port);
    info!("Device control features: Set Park Position, Calibrate IMU, Factory Reset, Manual Commands");
    
    if connection_manager.is_connected().await {
        if let Some(current_port) = connection_manager.get_current_port().await {
            info!("Connected to: {}", current_port);
        }
    } else {
        info!("Use the web interface to connect to your nRF52840 device");
    }
    
    info!("Press Ctrl+C to stop");
    
    // Wait for server or Ctrl+C
    tokio::select! {
        result = server_handle => {
            if let Err(e) = result {
                error!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down...");
            
            // Gracefully disconnect
            if connection_manager.is_connected().await {
                info!("Disconnecting from device...");
                if let Err(e) = connection_manager.disconnect().await {
                    error!("Error during shutdown disconnect: {}", e);
                }
            }
        }
    }
    
    Ok(())
}