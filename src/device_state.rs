use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    // Park sensor state
    pub connected: bool,
    pub is_safe: bool,  // In ASCOM terms, "safe" means parked
    pub current_pitch: f32,
    pub current_roll: f32,
    pub park_pitch: f32,
    pub park_roll: f32,
    pub position_tolerance: f32,
    pub last_update: u64,
    pub device_name: String,
    pub device_version: String,
    pub manufacturer: String,
    pub serial_port: Option<String>,
    pub error_message: Option<String>,
    
    // Telescope state
    pub telescope_connected: bool,
    pub telescope_url: Option<String>,
    pub telescope_device_number: u32,
    pub telescope_status: crate::telescope_client::TelescopeStatus,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceState {
    pub fn new() -> Self {
        Self {
            // Park sensor defaults
            connected: false,
            is_safe: false,
            current_pitch: 0.0,
            current_roll: 0.0,
            park_pitch: 0.0,
            park_roll: 0.0,
            position_tolerance: 2.0,
            last_update: 0,
            device_name: "Telescope Park Sensor".to_string(),
            device_version: "Unknown".to_string(),
            manufacturer: "Corey Smart".to_string(),
            serial_port: None,
            error_message: None,
            
            // Telescope defaults
            telescope_connected: false,
            telescope_url: None,
            telescope_device_number: 0,
            telescope_status: crate::telescope_client::TelescopeStatus::default(),
        }
    }
    
    pub fn update_timestamp(&mut self) {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
    
    pub fn set_error(&mut self, error: &str) {
        self.error_message = Some(error.to_string());
        self.connected = false;
        self.update_timestamp();
    }
    
    pub fn clear_error(&mut self) {
        self.error_message = None;
        self.update_timestamp();
    }
    
    pub fn is_recent(&self, max_age_seconds: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now.saturating_sub(self.last_update) <= max_age_seconds
    }
}