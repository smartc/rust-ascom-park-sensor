use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tracing::{info, error, debug};
use serde_json::json;

const DISCOVERY_PORT: u16 = 32227;
const DISCOVERY_MESSAGE: &str = "alpacadiscovery1";

pub async fn start_discovery_server(alpaca_port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind_addr = format!("0.0.0.0:{}", DISCOVERY_PORT);
    let socket = UdpSocket::bind(&bind_addr).await?;
    
    info!("ASCOM Alpaca discovery server listening on UDP {}", bind_addr);
    info!("Will respond with Alpaca port: {}", alpaca_port);
    
    let mut buf = [0; 1024];
    
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, addr)) => {
                let message = String::from_utf8_lossy(&buf[..len]);
                debug!("Received discovery message from {}: '{}'", addr, message.trim());
                
                if message.trim() == DISCOVERY_MESSAGE {
                    handle_discovery_request(&socket, addr, alpaca_port).await;
                } else {
                    debug!("Ignoring non-discovery message: '{}'", message.trim());
                }
            }
            Err(e) => {
                error!("Discovery server error: {}", e);
            }
        }
    }
}

async fn handle_discovery_request(socket: &UdpSocket, addr: SocketAddr, alpaca_port: u16) {
    debug!("Processing discovery request from {}", addr);
    
    // Create ASCOM Alpaca discovery response
    let response = json!({
        "AlpacaPort": alpaca_port
    });
    
    let response_str = response.to_string();
    
    match socket.send_to(response_str.as_bytes(), addr).await {
        Ok(bytes_sent) => {
            info!("Sent discovery response to {}: {} bytes", addr, bytes_sent);
            debug!("Discovery response: {}", response_str);
        }
        Err(e) => {
            error!("Failed to send discovery response to {}: {}", addr, e);
        }
    }
}