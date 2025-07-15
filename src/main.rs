// src/main.rs
// Add discovery server startup

mod device_state;
mod serial_client;
mod alpaca_server;
mod port_discovery;
mod connection_manager;
mod discovery_server;  // Add this line
mod errors;

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error, warn};
use tracing_subscriber;

use device_state::DeviceState;
use connection_manager::ConnectionManager;
use alpaca_server::create_alpaca_server;
use discovery_server::start_discovery_server;  // Add this line

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, help = "Serial port (e.g., COM3, /dev/ttyUSB0, /dev/ttyACM0)")]
    port: Option<String>,

    #[arg(short, long, default_value = "115200", help = "Baud rate for serial communication")]
    baud: u32,

    #[arg(long, default_value = "0.0.0.0", help = "HTTP server bind address")]
    bind: String,

    #[arg(long, default_value = "11111", help = "HTTP server port for ASCOM Alpaca")]
    http_port: u16,

    #[arg(long, help = "Auto-select first available nRF52840-like device")]
    auto: bool,

    #[arg(short, long, help = "Enable debug logging")]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Setup logging
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(if args.debug { 
            tracing::Level::DEBUG 
        } else { 
            tracing::Level::INFO 
        })
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)?;
    
    info!("nRF52840 Telescope Park Bridge v{} starting...", env!("CARGO_PKG_VERSION"));
    
    if args.debug {
        info!("Debug logging enabled");
    }
    
    // Note about UDP discovery port
    info!("Note: Discovery requires UDP port 32227 - may need firewall exception");
    
    // Initialize shared state
    let device_state = Arc::new(RwLock::new(DeviceState::new()));
    let connection_manager = Arc::new(ConnectionManager::new(device_state.clone()));
    
    // Determine target port
    let target_port = if let Some(port) = args.port {
        Some(port)
    } else if args.auto {
        match port_discovery::discover_ports() {
            Ok(ports) => {
                let mut found_port = None;
                
                // Look for nRF52840-like devices
                for port in &ports {
                    if port.description.to_lowercase().contains("usb") || 
                       port.description.to_lowercase().contains("serial") ||
                       port.description.to_lowercase().contains("xiao") ||
                       port.description.to_lowercase().contains("nrf52") {
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
    
    // Start the discovery server
    info!("Starting ASCOM Alpaca discovery server...");
    let discovery_handle = tokio::spawn(async move {
        if let Err(e) = start_discovery_server(args.http_port).await {
            error!("Discovery server error: {}", e);
        }
    });
    
    // Start the ASCOM Alpaca server
    info!("Starting ASCOM Alpaca server...");
    let server_handle = tokio::spawn(async move {
        if let Err(e) = create_alpaca_server(args.bind, args.http_port, device_state, connection_manager.clone()).await {
            error!("Failed to start ASCOM Alpaca server: {}", e);
        }
    });
    
    // Wait for either service to complete (they should run forever)
    tokio::select! {
        _ = discovery_handle => {
            warn!("Discovery server terminated");
        }
        _ = server_handle => {
            warn!("ASCOM Alpaca server terminated");
        }
    }
    
    Ok(())
}