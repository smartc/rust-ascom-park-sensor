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
#[command(about = "ASCOM Alpaca bridge for nRF52840 Telescope Park Sensor v0.4.0")]
#[command(version = "0.4.0")]
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
    
    info!("Starting Telescope Park Bridge v0.4.0");
    info!("FIXED: ASCOM Alpaca compliance - PUT Connected support added");
    info!("FIXED: HTTP MethodNotAllowed errors resolved");
    info!("NEW: Enhanced sensor communication error detection");
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
                        info!("Found potential nRF52840 device: {} ({})", port.name, port.description);
                        found_port = Some(port.name.clone());
                        break;
                    }
                }
                
                if found_port.is_none() {
                    // Fallback: use first available port
                    if let Some(first_port) = ports.first() {
                        info!("No nRF52840-like device found, using first available: {} ({})", 
                              first_port.name, first_port.description);
                        found_port = Some(first_port.name.clone());
                    }
                }
                
                found_port
            }
            Err(e) => {
                error!("Failed to discover ports: {}", e);
                None
            }
        }
    } else {
        None
    };
    
    // Auto-connect if port was specified or found
    if let Some(port) = target_port {
        info!("Attempting auto-connection to {}...", port);
        match connection_manager.connect(port.clone(), args.baud).await {
            Ok(_) => {
                info!("Successfully auto-connected to {}", port);
            }
            Err(e) => {
                error!("Auto-connection failed: {}. Bridge will start without device connection.", e);
                info!("Use the web interface to manually connect to your device.");
            }
        }
    } else {
        info!("No port specified. Use --port, --auto, or web interface to connect.");
    }
    
    // Start the ASCOM Alpaca server
    info!("Starting ASCOM Alpaca server...");
    if let Err(e) = create_alpaca_server(args.bind, args.http_port, device_state, connection_manager).await {
        error!("Failed to start ASCOM Alpaca server: {}", e);
        return Err(anyhow::anyhow!("Server error: {}", e));
    }
    
    Ok(())
}