use std::io::{self, Write};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_serial::SerialPortBuilderExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("nRF52840 Device Communication Test");
    println!("==================================");
    
    // Get port from user
    print!("Enter COM port (e.g. COM26): ");
    io::stdout().flush()?;
    let mut port_input = String::new();
    io::stdin().read_line(&mut port_input)?;
    let port_name = port_input.trim();
    
    println!("Connecting to {} at 115200 baud...", port_name);
    
    // Open serial port
    let port = tokio_serial::new(port_name, 115200)
        .timeout(Duration::from_millis(1000))
        .data_bits(tokio_serial::DataBits::Eight)
        .flow_control(tokio_serial::FlowControl::None)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .open_native_async()?;
    
    let (reader, mut writer) = tokio::io::split(port);
    let mut reader = BufReader::new(reader);
    
    println!("Connected! Waiting for device to be ready...");
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    // Test commands
    let test_commands = vec![
        ("01", "Get Status"),
        ("02", "Get Position"), 
        ("03", "Check Parked"),
        ("05", "Get Park Position"),
        ("08", "Get Version"),
    ];
    
    for (cmd, desc) in test_commands {
        println!("\n--- Testing: {} ({}) ---", desc, cmd);
        
        // Send command
        let command_str = format!("<{}>\n", cmd);
        println!("Sending: {}", command_str.trim());
        writer.write_all(command_str.as_bytes()).await?;
        writer.flush().await?;
        
        // Wait for response with timeout
        let mut responses = Vec::new();
        let start_time = std::time::Instant::now();
        
        while start_time.elapsed() < Duration::from_secs(3) {
            match tokio::time::timeout(Duration::from_millis(500), reader.read_line(&mut String::new())).await {
                Ok(Ok(_)) => {
                    let mut line = String::new();
                    if let Ok(_) = reader.read_line(&mut line).await {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            println!("Response: {}", trimmed);
                            responses.push(trimmed.to_string());
                            
                            // If it looks like JSON and contains "status", we probably got our response
                            if trimmed.contains("\"status\"") {
                                break;
                            }
                        }
                    }
                }
                _ => break, // Timeout or error
            }
        }
        
        if responses.is_empty() {
            println!("No response received");
        }
        
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    
    println!("\nTest complete. Press Enter to exit...");
    let mut _input = String::new();
    io::stdin().read_line(&mut _input)?;
    
    Ok(())
}