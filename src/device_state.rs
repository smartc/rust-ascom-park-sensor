use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    // Connection status
    pub connected: bool,
    pub serial_port: Option<String>,
    pub error_message: Option<String>,
    pub last_update: u64,
    
    // Device information (from firmware)
    pub device_name: String,
    pub device_version: String,
    pub manufacturer: String,
    pub platform: String,
    pub imu: String,
    
    // Position data (from firmware)
    pub current_pitch: f32,
    pub current_roll: f32,
    pub park_pitch: f32,
    pub park_roll: f32,
    pub position_tolerance: f32,
    
    // Park status (from firmware)
    pub is_parked: bool,
    pub is_safe: bool,  // ASCOM safety monitor compatibility (same as is_parked)
    
    // Calibration status
    pub is_calibrated: bool,
    
    // Device capabilities
    pub has_builtin_imu: bool,
    pub storage_available: bool,
    
    // System info
    pub uptime: u64,
    pub free_heap: u64,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceState {
    pub fn new() -> Self {
        Self {
            // Connection defaults
            connected: false,
            serial_port: None,
            error_message: None,
            last_update: 0,
            
            // Device defaults
            device_name: "Telescope Park Sensor".to_string(),
            device_version: "Unknown".to_string(),
            manufacturer: "Corey Smart".to_string(),
            platform: "nRF52840 XIAO Sense".to_string(),
            imu: "LSM6DS3TR-C".to_string(),
            
            // Position defaults
            current_pitch: 0.0,
            current_roll: 0.0,
            park_pitch: 0.0,
            park_roll: 0.0,
            position_tolerance: 2.0,
            
            // Status defaults
            is_parked: false,
            is_safe: false,
            is_calibrated: false,
            
            // Capabilities
            has_builtin_imu: true,
            storage_available: false,  // nRF52840 with mbed core has limited storage
            
            // System defaults
            uptime: 0,
            free_heap: 0,
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
    
    // Update device state from firmware status response
    pub fn update_from_status(&mut self, status_data: &StatusResponse) {
        self.device_name = status_data.device_name.clone();
        self.device_version = status_data.version.clone();
        self.manufacturer = status_data.manufacturer.clone();
        self.platform = status_data.platform.clone().unwrap_or_else(|| "nRF52840".to_string());
        self.imu = status_data.imu.clone().unwrap_or_else(|| "LSM6DS3TR-C".to_string());
        self.is_parked = status_data.parked;
        self.is_safe = status_data.parked; // For ASCOM compatibility
        self.is_calibrated = status_data.calibrated;
        self.uptime = status_data.uptime;
        self.update_timestamp();
    }
    
    // Update position data from firmware position response
    pub fn update_from_position(&mut self, position_data: &PositionResponse) {
        self.current_pitch = position_data.pitch;
        self.current_roll = position_data.roll;
        self.update_timestamp();
    }
    
    // Update park status from firmware park response
    pub fn update_from_park_status(&mut self, park_data: &ParkStatusResponse) {
        self.is_parked = park_data.parked;
        self.is_safe = park_data.parked; // For ASCOM compatibility
        self.current_pitch = park_data.current_pitch;
        self.current_roll = park_data.current_roll;
        self.park_pitch = park_data.park_pitch;
        self.park_roll = park_data.park_roll;
        self.position_tolerance = park_data.tolerance;
        self.update_timestamp();
    }
}

// Firmware response structures based on the serial_interface.cpp
#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    #[serde(rename = "deviceName")]
    pub device_name: String,
    pub version: String,
    pub manufacturer: String,
    pub platform: Option<String>,
    pub imu: Option<String>,
    pub parked: bool,
    pub calibrated: bool,
    #[serde(rename = "ledStatus")]
    pub led_status: bool,
    pub uptime: u64,
}

#[derive(Debug, Deserialize)]
pub struct PositionResponse {
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Deserialize)]
pub struct ParkStatusResponse {
    pub parked: bool,
    #[serde(rename = "currentPitch")]
    pub current_pitch: f32,
    #[serde(rename = "currentRoll")]
    pub current_roll: f32,
    #[serde(rename = "parkPitch")]
    pub park_pitch: f32,
    #[serde(rename = "parkRoll")]
    pub park_roll: f32,
    pub tolerance: f32,
    #[serde(rename = "pitchDiff")]
    pub pitch_diff: f32,
    #[serde(rename = "rollDiff")]
    pub roll_diff: f32,
}

#[derive(Debug, Deserialize)]
pub struct FirmwareResponse {
    pub status: String,
    pub data: Option<serde_json::Value>,
    pub message: Option<String>,
    pub command: Option<String>,
}