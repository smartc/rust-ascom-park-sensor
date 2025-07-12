// src/connection_manager.rs
use crate::device_state::DeviceState;
use crate::errors::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

#[derive(Debug)]
pub struct ConnectionInfo {
    pub port: String,
    pub baud_rate: u32,
}

pub struct ConnectionManager {
    device_state: Arc<RwLock<DeviceState>>,
    current_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    current_cancellation: Arc<RwLock<Option<CancellationToken>>>,
    current_connection: Arc<RwLock<Option<ConnectionInfo>>>,
}

impl ConnectionManager {
    pub fn new(device_state: Arc<RwLock<DeviceState>>) -> Self {
        Self {
            device_state,
            current_task: Arc::new(RwLock::new(None)),
            current_cancellation: Arc::new(RwLock::new(None)),
            current_connection: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn connect(&self, port: String, baud_rate: u32) -> Result<String> {
        info!("ConnectionManager: Connecting to {} at {} baud", port, baud_rate);

        // First, disconnect any existing connection
        self.disconnect_internal().await;

        // Create new cancellation token
        let cancel_token = CancellationToken::new();
        {
            let mut current_cancel = self.current_cancellation.write().await;
            *current_cancel = Some(cancel_token.clone());
        }

        // Start new serial connection task
        let device_state_clone = self.device_state.clone();
        let port_clone = port.clone();
        
        let new_task = tokio::spawn(async move {
            if let Err(e) = crate::serial_client::run_serial_client_with_cancellation(
                port_clone,
                baud_rate,
                device_state_clone,
                cancel_token,
            ).await {
                tracing::error!("Serial client error: {}", e);
            }
        });

        // Store the new task and connection info
        {
            let mut current_task = self.current_task.write().await;
            *current_task = Some(new_task);
        }

        {
            let mut current_conn = self.current_connection.write().await;
            *current_conn = Some(ConnectionInfo {
                port: port.clone(),
                baud_rate,
            });
        }

        // Update device state
        {
            let mut device_state = self.device_state.write().await;
            device_state.serial_port = Some(port.clone());
            device_state.clear_error();
        }

        Ok(format!("Connecting to nRF52840 device on {} at {} baud", port, baud_rate))
    }

    pub async fn disconnect(&self) -> Result<String> {
        info!("ConnectionManager: Disconnecting from device");
        self.disconnect_internal().await;
        
        // Reset device state to disconnected defaults
        {
            let mut device_state = self.device_state.write().await;
            device_state.reset_to_disconnected();
        }

        Ok("Disconnected from nRF52840 device and cleared all data".to_string())
    }

    async fn disconnect_internal(&self) {
        // Cancel the current operation
        let cancel_token = {
            let mut current_cancel = self.current_cancellation.write().await;
            current_cancel.take()
        };

        if let Some(cancel_token) = cancel_token {
            info!("ConnectionManager: Cancelling serial operations");
            cancel_token.cancel();
        }

        // Abort the current task
        let task_to_abort = {
            let mut current_task = self.current_task.write().await;
            current_task.take()
        };

        if let Some(task) = task_to_abort {
            info!("ConnectionManager: Aborting serial task");
            task.abort();
            match tokio::time::timeout(Duration::from_millis(2000), task).await {
                Ok(_) => info!("ConnectionManager: Serial task stopped cleanly"),
                Err(_) => warn!("ConnectionManager: Serial task abort timed out"),
            }
        }

        // Clear connection info
        {
            let mut current_conn = self.current_connection.write().await;
            *current_conn = None;
        }

        // Give time for cleanup
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    pub async fn is_connected(&self) -> bool {
        let device_state = self.device_state.read().await;
        device_state.connected
    }

    pub async fn get_current_connection(&self) -> Option<ConnectionInfo> {
        let current_conn = self.current_connection.read().await;
        current_conn.as_ref().map(|conn| ConnectionInfo {
            port: conn.port.clone(),
            baud_rate: conn.baud_rate,
        })
    }

    pub async fn get_current_port(&self) -> Option<String> {
        let current_conn = self.current_connection.read().await;
        current_conn.as_ref().map(|conn| conn.port.clone())
    }
}

impl Drop for ConnectionManager {
    fn drop(&mut self) {
        // We can't await in drop, but we can spawn a task to clean up
        // This is best-effort cleanup
        info!("ConnectionManager: Dropping, attempting cleanup");
    }
}