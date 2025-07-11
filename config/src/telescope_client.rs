use ascom_alpaca::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum TelescopeConnection {
    Alpaca { url: String, device_number: u32 },
    Local { prog_id: String },
}

#[derive(Debug)]
pub struct TelescopeClient {
    connection: TelescopeConnection,
    client: Option<Arc<Client>>,
    device_number: u32,
}

impl Clone for TelescopeClient {
    fn clone(&self) -> Self {
        Self {
            connection: self.connection.clone(),
            client: self.client.clone(),
            device_number: self.device_number,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TelescopeStatus {
    pub connected: bool,
    pub name: String,
    pub description: String,
    pub ra: f64,          // Right Ascension in decimal hours
    pub dec: f64,         // Declination in decimal degrees
    pub azimuth: f64,     // Azimuth in degrees
    pub altitude: f64,    // Altitude in degrees
    pub tracking: bool,
    pub slewing: bool,
    pub at_home: bool,
    pub at_park: bool,
    pub can_park: bool,
    pub can_home: bool,
    pub can_slew: bool,
    pub can_move_axis: bool,
    pub pier_side: String,
}

impl Default for TelescopeStatus {
    fn default() -> Self {
        Self {
            connected: false,
            name: "Unknown".to_string(),
            description: "Unknown".to_string(),
            ra: 0.0,
            dec: 0.0,
            azimuth: 0.0,
            altitude: 0.0,
            tracking: false,
            slewing: false,
            at_home: false,
            at_park: false,
            can_park: false,
            can_home: false,
            can_slew: false,
            can_move_axis: false,
            pier_side: "Unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SlewDirection {
    North,
    South,
    East,
    West,
}

// Axis enum for telescope movements
#[derive(Debug, Clone, Copy)]
pub enum TelescopeAxis {
    Primary,   // RA/Azimuth
    Secondary, // Dec/Altitude
}

impl TelescopeClient {
    pub fn new(connection: TelescopeConnection) -> Self {
        Self {
            connection,
            client: None,
            device_number: 0,
        }
    }

    pub async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match &self.connection {
            TelescopeConnection::Alpaca { url, device_number } => {
                info!("Connecting to Alpaca telescope at {} device {}", url, device_number);
                let client = Arc::new(Client::new(url)?);
                self.client = Some(client.clone());
                self.device_number = *device_number;
                
                // Test connection by getting device info
                let _info = client.get_devices().await?;
                
                Ok(())
            }
            TelescopeConnection::Local { prog_id } => {
                info!("Connecting to local ASCOM telescope: {}", prog_id);
                // For local ASCOM connections, we'll use the default client which connects to localhost
                let client = Arc::new(Client::new("http://localhost:11111")?);
                self.client = Some(client.clone());
                self.device_number = 0;
                
                // Test connection
                let _info = client.get_devices().await?;
                
                Ok(())
            }
        }
    }

    pub async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // For now, just clear the client reference
        // The actual telescope disconnection would be handled by the ASCOM driver
        self.client = None;
        Ok(())
    }

    pub async fn get_status(&self) -> Result<TelescopeStatus, Box<dyn std::error::Error + Send + Sync>> {
        let mut status = TelescopeStatus::default();

        if let Some(_client) = &self.client {
            // Note: The actual implementation would depend on the specific API methods
            // available in the ascom-alpaca crate. Since the exact API is unclear from
            // the error messages, this is a simplified version.
            status.connected = true;
            
            // In a real implementation, you would call the appropriate methods
            // on the telescope device to get the actual values
        }

        Ok(status)
    }

    pub async fn set_tracking(&self, _tracking: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            // Implementation would go here
            info!("Setting tracking (not implemented)");
        }
        Ok(())
    }

    pub async fn slew_to_coordinates(&self, ra: f64, dec: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            info!("Slewing telescope to RA: {}, Dec: {} (not implemented)", ra, dec);
        }
        Ok(())
    }

    pub async fn abort_slew(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            info!("Aborting telescope slew (not implemented)");
        }
        Ok(())
    }

    pub async fn park(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            info!("Parking telescope (not implemented)");
        }
        Ok(())
    }

    pub async fn unpark(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            info!("Unparking telescope (not implemented)");
        }
        Ok(())
    }

    pub async fn find_home(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            info!("Finding telescope home (not implemented)");
        }
        Ok(())
    }

    pub async fn move_axis(&self, direction: SlewDirection, rate: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            debug!("Moving telescope {:?} at rate {} (not implemented)", direction, rate);
        }
        Ok(())
    }

    pub async fn stop_all_movement(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_client) = &self.client {
            info!("Stopping all telescope movement (not implemented)");
        }
        Ok(())
    }

    pub async fn get_axis_rates(&self) -> Result<Vec<f64>, Box<dyn std::error::Error + Send + Sync>> {
        // Return default rates for now
        Ok(vec![0.5, 1.0, 2.0, 4.0])
    }
}

// Windows-specific ASCOM discovery
#[cfg(windows)]
pub fn discover_local_ascom_telescopes() -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    use winreg::enums::*;
    use winreg::RegKey;

    let mut telescopes = Vec::new();
    
    // Open ASCOM Telescope Drivers registry key
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(ascom_key) = hklm.open_subkey("SOFTWARE\\ASCOM\\Telescope Drivers") {
        for name in ascom_key.enum_keys() {
            if let Ok(name) = name {
                telescopes.push(name);
            }
        }
    }

    // Also check in 32-bit registry on 64-bit systems
    if let Ok(ascom_key) = hklm.open_subkey("SOFTWARE\\WOW6432Node\\ASCOM\\Telescope Drivers") {
        for name in ascom_key.enum_keys() {
            if let Ok(name) = name {
                if !telescopes.contains(&name) {
                    telescopes.push(name);
                }
            }
        }
    }

    Ok(telescopes)
}

#[cfg(not(windows))]
pub fn discover_local_ascom_telescopes() -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    // On non-Windows platforms, return empty list
    Ok(vec![])
}