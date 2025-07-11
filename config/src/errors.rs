use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Serial communication error: {0}")]
    Serial(#[from] tokio_serial::Error),
    
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Device not connected")]
    NotConnected,
    
    #[error("Invalid response from device: {0}")]
    InvalidResponse(String),
    
    #[error("Timeout waiting for device response")]
    Timeout,
    
    #[error("Device error: {0}")]
    Device(String),
}

pub type Result<T> = std::result::Result<T, BridgeError>;