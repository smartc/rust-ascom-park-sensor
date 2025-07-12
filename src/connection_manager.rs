// src/connection_manager.rs
use crate::device_state::DeviceState;
use crate::errors::{Result, BridgeError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, debug, error};

#[derive(Debug)]
pub struct ConnectionInfo {
    pub port: String,
    pub baud_rate: u32,
}

#[derive(Debug)]
pub struct CommandRequest {
    pub command: String,
    pub response_sender: oneshot::Sender<Result<String>>,
}

pub struct ConnectionManager {
    device_state: Arc<RwLock<DeviceState>>,
    current_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    current_cancellation: Arc<RwLock<Option<CancellationToken>>>,
    current_connection: Arc<RwLock<Option<ConnectionInfo>>>,
    command_sender: Arc<RwLock<Option<mpsc::UnboundedSender<CommandRequest>>>>,
}

impl ConnectionManager {
    pub fn new(device_state: Arc<RwLock<DeviceState>>) -> Self {
        Self {
            device_state,
            current_task: Arc::new(RwLock::new(None)),
            current_cancellation: Arc::new(RwLock::new(None)),
            current_connection: Arc::new(RwLock::new(None)),
            command_sender: Arc::new(RwLock::new(None)),
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

        // Create command channel
        let (cmd_sender, cmd_receiver) = mpsc::unbounded_channel::<CommandRequest>();
        {
            let mut current_cmd_sender = self.command_sender.write().await;
            *current_cmd_sender = Some(cmd_sender);
        }

        // Start new serial connection task with command support
        let device_state_clone = self.device_state.clone();
        let port_clone = port.clone();
        
        let new_task = tokio::spawn(async move {
            if let Err(e) = crate::serial_client::run_serial_client_with_commands(
                port_clone,
                baud_rate,
                device_state_clone,
                cancel_token,
                cmd_receiver,
            ).await {
                error!("Serial client error: {}", e);
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
        // Clear command sender first
        {
            let mut cmd_sender = self.command_sender.write().await;
            *cmd_sender = None;
        }

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

    pub async fn send_command(&self, command: &str) -> Result<String> {
        let cmd_sender = {
            let cmd_sender_guard = self.command_sender.read().await;
            cmd_sender_guard.clone()
        };

        let sender = cmd_sender.ok_or_else(|| {
            BridgeError::NotConnected
        })?;

        debug!("ConnectionManager: Sending command: {}", command);

        let (response_sender, response_receiver) = oneshot::channel();
        let cmd_request = CommandRequest {
            command: command.to_string(),
            response_sender,
        };

        sender.send(cmd_request).map_err(|_| {
            BridgeError::Device("Command channel closed".to_string())
        })?;

        // Wait for response with timeout - now waits for actual data response, not just ACK
        match tokio::time::timeout(Duration::from_secs(15), response_receiver).await {
            Ok(Ok(result)) => {
                debug!("ConnectionManager: Command response received");
                result
            }
            Ok(Err(_)) => {
                error!("ConnectionManager: Command response channel closed");
                Err(BridgeError::Device("Command response channel closed".to_string()))
            }
            Err(_) => {
                error!("ConnectionManager: Command timeout");
                Err(BridgeError::Timeout)
            }
        }
    }

    pub async fn calibrate_sensor(&self) -> Result<String> {
        info!("ConnectionManager: Starting sensor calibration");
        self.send_command("06").await
    }

    pub async fn set_park_position(&self) -> Result<String> {
        info!("ConnectionManager: Setting park position");
        self.send_command("0D").await // Use software set park command
    }

    pub async fn factory_reset(&self) -> Result<String> {
        info!("ConnectionManager: Performing factory reset");
        self.send_command("0E").await
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