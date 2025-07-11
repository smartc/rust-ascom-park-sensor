use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use url::Url;

#[derive(Debug, Clone)]
pub struct TelescopeClient {
    client: Client,
    base_url: String,
    device_number: u32,
    client_id: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AlpacaResponse<T> {
    #[serde(rename = "Value")]
    pub value: T,
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_id: u32,
    #[serde(rename = "ServerTransactionID")]
    pub server_transaction_id: u32,
    #[serde(rename = "ErrorNumber")]
    pub error_number: u32,
    #[serde(rename = "ErrorMessage")]
    pub error_message: String,
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
            pier_side: "Unknown".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConnectedRequest {
    #[serde(rename = "Connected")]
    pub connected: bool,
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_id: u32,
}

#[derive(Debug, Serialize)]
pub struct SlewRequest {
    #[serde(rename = "RightAscension")]
    pub ra: f64,
    #[serde(rename = "Declination")]
    pub dec: f64,
    #[serde(rename = "ClientTransactionID")]
    pub client_transaction_id: u32,
}

impl TelescopeClient {
    pub fn new(base_url: String, device_number: u32) -> Self {
        Self {
            client: Client::new(),
            base_url,
            device_number,
            client_id: 42, // Static client ID for now
        }
    }

    fn build_url(&self, endpoint: &str) -> Result<Url, url::ParseError> {
        let url_str = format!(
            "{}/api/v1/telescope/{}/{}",
            self.base_url.trim_end_matches('/'),
            self.device_number,
            endpoint
        );
        Url::parse(&url_str)
    }

    pub async fn get_connected(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("connected")?;
        debug!("Getting telescope connected status from: {}", url);

        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    pub async fn set_connected(&self, connected: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("connected")?;
        debug!("Setting telescope connected to {} at: {}", connected, url);

        let request = ConnectedRequest {
            connected,
            client_transaction_id: self.client_id,
        };

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&request)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope connect error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope connection set to: {}", connected);
        Ok(())
    }

    pub async fn get_status(&self) -> Result<TelescopeStatus, Box<dyn std::error::Error + Send + Sync>> {
        let mut status = TelescopeStatus::default();

        // Get basic connection status
        status.connected = self.get_connected().await.unwrap_or(false);

        if !status.connected {
            return Ok(status);
        }

        // Get all telescope properties
        if let Ok(name) = self.get_name().await {
            status.name = name;
        }

        if let Ok(description) = self.get_description().await {
            status.description = description;
        }

        if let Ok(ra) = self.get_right_ascension().await {
            status.ra = ra;
        }

        if let Ok(dec) = self.get_declination().await {
            status.dec = dec;
        }

        if let Ok(az) = self.get_azimuth().await {
            status.azimuth = az;
        }

        if let Ok(alt) = self.get_altitude().await {
            status.altitude = alt;
        }

        if let Ok(tracking) = self.get_tracking().await {
            status.tracking = tracking;
        }

        if let Ok(slewing) = self.get_slewing().await {
            status.slewing = slewing;
        }

        if let Ok(at_home) = self.get_at_home().await {
            status.at_home = at_home;
        }

        if let Ok(at_park) = self.get_at_park().await {
            status.at_park = at_park;
        }

        if let Ok(can_park) = self.get_can_park().await {
            status.can_park = can_park;
        }

        if let Ok(can_home) = self.get_can_find_home().await {
            status.can_home = can_home;
        }

        if let Ok(can_slew) = self.get_can_slew().await {
            status.can_slew = can_slew;
        }

        if let Ok(pier_side) = self.get_side_of_pier().await {
            status.pier_side = format!("{:?}", pier_side);
        }

        Ok(status)
    }

    // Individual property getters
    async fn get_name(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("name")?;
        let response: AlpacaResponse<String> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_description(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("description")?;
        let response: AlpacaResponse<String> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_right_ascension(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("rightascension")?;
        let response: AlpacaResponse<f64> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_declination(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("declination")?;
        let response: AlpacaResponse<f64> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_azimuth(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("azimuth")?;
        let response: AlpacaResponse<f64> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_altitude(&self) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("altitude")?;
        let response: AlpacaResponse<f64> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_tracking(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("tracking")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_slewing(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("slewing")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_at_home(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("athome")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_at_park(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("atpark")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_can_park(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("canpark")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_can_find_home(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("canfindhome")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_can_slew(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("canslew")?;
        let response: AlpacaResponse<bool> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    async fn get_side_of_pier(&self) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("sideofpier")?;
        let response: AlpacaResponse<i32> = self
            .client
            .get(url)
            .query(&[("ClientTransactionID", self.client_id)])
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            return Err(response.error_message.into());
        }

        Ok(response.value)
    }

    // Telescope control methods
    pub async fn set_tracking(&self, tracking: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("tracking")?;
        debug!("Setting telescope tracking to {} at: {}", tracking, url);

        let mut form = HashMap::new();
        form.insert("Tracking", tracking.to_string());
        form.insert("ClientTransactionID", self.client_id.to_string());

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope tracking error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope tracking set to: {}", tracking);
        Ok(())
    }

    pub async fn slew_to_coordinates(&self, ra: f64, dec: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("slewtocoordinates")?;
        debug!("Slewing telescope to RA: {}, Dec: {} at: {}", ra, dec, url);

        let mut form = HashMap::new();
        form.insert("RightAscension", ra.to_string());
        form.insert("Declination", dec.to_string());
        form.insert("ClientTransactionID", self.client_id.to_string());

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope slew error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope slewing to RA: {}, Dec: {}", ra, dec);
        Ok(())
    }

    pub async fn abort_slew(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("abortslew")?;
        debug!("Aborting telescope slew at: {}", url);

        let mut form = HashMap::new();
        form.insert("ClientTransactionID", self.client_id.to_string());

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope abort error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope slew aborted");
        Ok(())
    }

    pub async fn park(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("park")?;
        debug!("Parking telescope at: {}", url);

        let mut form = HashMap::new();
        form.insert("ClientTransactionID", self.client_id.to_string());

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope park error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope parking");
        Ok(())
    }

    pub async fn unpark(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("unpark")?;
        debug!("Unparking telescope at: {}", url);

        let mut form = HashMap::new();
        form.insert("ClientTransactionID", self.client_id.to_string());

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope unpark error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope unparking");
        Ok(())
    }

    pub async fn find_home(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.build_url("findhome")?;
        debug!("Finding telescope home at: {}", url);

        let mut form = HashMap::new();
        form.insert("ClientTransactionID", self.client_id.to_string());

        let response: AlpacaResponse<()> = self
            .client
            .put(url)
            .form(&form)
            .send()
            .await?
            .json()
            .await?;

        if response.error_number != 0 {
            error!("Telescope find home error: {}", response.error_message);
            return Err(response.error_message.into());
        }

        info!("Telescope finding home");
        Ok(())
    }
}