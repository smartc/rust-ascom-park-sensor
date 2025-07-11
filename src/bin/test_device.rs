use std::io::{self, Write};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_serial::SerialPortBuilderExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("nRF52840 Device Communication Test - With DTR/RTS Control");
    println!("=========================================================");
    
    // Get port from user
    print!("Enter COM port (e.g. COM26): ");
    io::stdout().flush()?;
    let mut port_input = String::new();
    io::stdin().read_line(&mut port_input)?;
    let port_name = port_input.trim();
    
    println!("Connecting to {} at 115200 baud...", port_name);
    
    // Open serial port
    let mut port = tokio_serial::new(port_name, 115200)
        .timeout(Duration::from_millis(1000))
        .data_bits(tokio_serial::DataBits::Eight)
        .flow_control(tokio_serial::FlowControl::None)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .open_native_async()?;
    
    println!("Port opened, setting DTR/RTS control signals...");
    
    // Try setting DTR and RTS like Arduino might
    #[cfg(windows)]
    {
        use tokio_serial::SerialPort;
        match port.write_data_terminal_ready(true) {
            Ok(_) => println!("DTR set to true"),
            Err(e) => println!("Failed to set DTR: {}", e),
        }
        match port.write_request_to_send(false) {
            Ok(_) => println!("RTS set to false"),
            Err(e) => println!("Failed to set RTS: {}", e),
        }
    }
    
    // Wait a moment for device to respond to DTR/RTS changes
    println!("Waiting for device to respond to control signals...");
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let (reader, mut writer) = tokio::io::split(port);
    let mut reader = BufReader::new(reader);
    
    println!("Connected! Reading startup messages...");
    
    // Read startup messages for longer period
    let mut startup_lines = 0;
    let start_time = std::time::Instant::now();
    
    while start_time.elapsed() < Duration::from_secs(5) && startup_lines < 100 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_millis(100), reader.read_line(&mut line)).await {
            Ok(Ok(bytes_read)) => {
                if bytes_read > 0 {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        startup_lines += 1;
                        println!("STARTUP {}: {}", startup_lines, trimmed);
                        
                        // Look for specific startup messages
                        if trimmed.contains("Device ready") || 
                           trimmed.contains("Setup complete") ||
                           trimmed.contains("Available Commands") {
                            println!("*** Device startup detected! ***");
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                println!("Read error: {}", e);
                break;
            }
            Err(_) => {
                // Timeout - continue reading
                continue;
            }
        }
    }
    
    if startup_lines == 0 {
        println!("*** NO STARTUP MESSAGES RECEIVED ***");
        println!("This suggests a DTR/RTS or timing issue.");
    } else {
        println!("*** Received {} startup lines ***", startup_lines);
    }
    
    println!("\n=== Testing Commands ===");
    
    // Wait a bit more
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    // Test simple command with LF ending (matching Arduino "New Line")
    println!("Sending <00> command (help)...");
    writer.write_all(b"<00>\n").await?;
    writer.flush().await?;
    
    // Read response
    let mut response_count = 0;
    let start_time = std::time::Instant::now();
    
    while start_time.elapsed() < Duration::from_secs(5) {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_millis(200), reader.read_line(&mut line)).await {
            Ok(Ok(bytes_read)) => {
                if bytes_read > 0 {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        response_count += 1;
                        println!("RESPONSE {}: {}", response_count, trimmed);
                    }
                }
            }
            Ok(Err(e)) => {
                println!("Read error: {}", e);
                break;
            }
            Err(_) => continue,
        }
    }
    
    if response_count == 0 {
        println!("*** NO RESPONSE TO <00> COMMAND ***");
    }
    
    println!("\n=== Manual Command Test ===");
    println!("Enter commands manually (or 'quit' to exit):");
    
    loop {
        print!("\nCommand: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let cmd = input.trim();
        
        if cmd.eq_ignore_ascii_case("quit") || cmd.is_empty() {
            break;
        }
        
        let command_str = format!("<{}>\n", cmd);
        println!("Sending: {}", command_str.trim());
        
        writer.write_all(command_str.as_bytes()).await?;
        writer.flush().await?;
        
        // Read responses
        let start_time = std::time::Instant::now();
        let mut got_response = false;
        
        while start_time.elapsed() < Duration::from_secs(3) {
            let mut line = String::new();
            match tokio::time::timeout(Duration::from_millis(200), reader.read_line(&mut line)).await {
                Ok(Ok(bytes_read)) => {
                    if bytes_read > 0 {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            println!("Response: {}", trimmed);
                            got_response = true;
                        }
                    }
                }
                _ => continue,
            }
        }
        
        if !got_response {
            println!("No response received");
        }
    }
    
    println!("Test complete!");
    Ok(())
}