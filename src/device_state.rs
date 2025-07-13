// src/device_state.rs
// Fixed version with proper nRF52840 response parsing and state management

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
    
    // ASCOM client connection state (separate from hardware)
    pub ascom_connected: bool,
    
    // Unique device identifier
    pub unique_id: String,
}

// Firmware response structures to match nRF52840 JSON output
#[derive(Debug, Deserialize)]
pub struct FirmwareResponse {
    pub status: String,  // "ack", "ok", "error"
    pub command: Option<String>,
    pub data: Option<serde_json::Value>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub parked: bool,
    pub calibrated: bool,
    #[serde(rename = "parkPitch")]
    pub park_pitch: f32,
    #[serde(rename = "parkRoll")]
    pub park_roll: f32,
    pub tolerance: f32,
    pub uptime: Option<u64>,
    #[serde(rename = "freeHeap")]
    pub free_heap: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct PositionResponse {
    pub pitch: f32,
    pub roll: f32,
    #[serde(default)]
    pub timestamp: u64,
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
    pub pitch_diff: Option<f32>,
    #[serde(rename = "rollDiff")]
    pub roll_diff: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct VersionResponse {
    #[serde(rename = "firmwareVersion")]
    pub firmware_version: String,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    pub manufacturer: String,
    pub platform: String,
    pub imu: String,
    #[serde(rename = "bluetoothReady")]
    pub bluetooth_ready: Option<bool>,
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
            storage_available: true,
            
            // System defaults
            uptime: 0,
            free_heap: 0,
            
            // ASCOM defaults
            ascom_connected: false,
            
            // Generate unique ID using UUID
            unique_id: uuid::Uuid::new_v4().to_string(),
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
    
    pub fn reset_to_disconnected(&mut self) {
        self.connected = false;
        self.serial_port = None;
        self.error_message = None;
        self.current_pitch = 0.0;
        self.current_roll = 0.0;
        self.is_parked = false;
        self.is_safe = false;
        self.update_timestamp();
    }
    
    pub fn is_recent(&self, max_age_seconds: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now.saturating_sub(self.last_update) <= max_age_seconds
    }
    
    // Update methods for different firmware response types
    pub fn update_from_status(&mut self, status: &StatusResponse) {
        self.is_parked = status.parked;
        self.is_safe = status.parked; // ASCOM Safety Monitor compatibility
        self.is_calibrated = status.calibrated;
        self.park_pitch = status.park_pitch;
        self.park_roll = status.park_roll;
        self.position_tolerance = status.tolerance;
        
        if let Some(uptime) = status.uptime {
            self.uptime = uptime;
        }
        
        if let Some(free_heap) = status.free_heap {
            self.free_heap = free_heap;
        }
        
        self.connected = true;
        self.clear_error();
        self.update_timestamp();
    }
    
    pub fn update_from_position(&mut self, position: &PositionResponse) {
        self.current_pitch = position.pitch;
        self.current_roll = position.roll;
        self.connected = true;
        self.clear_error();
        self.update_timestamp();
    }
    
    pub fn update_from_park_status(&mut self, park_status: &ParkStatusResponse) {
        self.is_parked = park_status.parked;
        self.is_safe = park_status.parked; // ASCOM Safety Monitor compatibility
        self.current_pitch = park_status.current_pitch;
        self.current_roll = park_status.current_roll;
        self.park_pitch = park_status.park_pitch;
        self.park_roll = park_status.park_roll;
        self.position_tolerance = park_status.tolerance;
        self.connected = true;
        self.clear_error();
        self.update_timestamp();
    }
    
    pub fn update_from_version(&mut self, version: &VersionResponse) {
        self.device_version = version.firmware_version.clone();
        self.device_name = version.device_name.clone();
        self.manufacturer = version.manufacturer.clone();
        self.platform = version.platform.clone();
        self.imu = version.imu.clone();
        self.connected = true;
        self.clear_error();
        self.update_timestamp();
    }
    
    // Calculate position difference from park position
    pub fn position_difference(&self) -> (f32, f32) {
        let pitch_diff = (self.current_pitch - self.park_pitch).abs();
        let roll_diff = (self.current_roll - self.park_roll).abs();
        (pitch_diff, roll_diff)
    }
    
    // Check if within tolerance (matches firmware logic)
    pub fn is_within_tolerance(&self) -> bool {
        let (pitch_diff, roll_diff) = self.position_difference();
        pitch_diff <= self.position_tolerance && roll_diff <= self.position_tolerance
    }
    
    // Get connection status summary for web interface
    pub fn connection_summary(&self) -> String {
        if !self.connected {
            if let Some(ref error) = self.error_message {
                format!("Disconnected: {}", error)
            } else {
                "Disconnected".to_string()
            }
        } else if self.is_recent(30) {
            "Connected".to_string()
        } else {
            "Connected (stale data)".to_string()
        }
    }
    
    // Get park status summary for web interface
    pub fn park_status_summary(&self) -> String {
        if !self.connected {
            "Unknown".to_string()
        } else if self.is_parked {
            "Parked".to_string()
        } else {
            let (pitch_diff, roll_diff) = self.position_difference();
            format!("Not Parked (P:{:.1}°, R:{:.1}°)", pitch_diff, roll_diff)
        }
    }
}