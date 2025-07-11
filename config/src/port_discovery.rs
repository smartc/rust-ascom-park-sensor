use anyhow::Result;
use serialport::SerialPortType;
use serde::Serialize;
use std::io::{self, Write};

#[derive(Debug, Clone, Serialize)]
pub struct PortInfo {
    pub name: String,
    pub description: String,
    pub manufacturer: Option<String>,
}

pub fn discover_ports() -> Result<Vec<PortInfo>> {
    let ports = serialport::available_ports()?;
    
    let mut discovered_ports = Vec::new();
    
    for port in ports {
        let description = match &port.port_type {
            SerialPortType::UsbPort(usb_info) => {
                format!("USB Device (VID: {:04X}, PID: {:04X})", 
                    usb_info.vid, usb_info.pid)
            }
            SerialPortType::BluetoothPort => "Bluetooth Port".to_string(),
            SerialPortType::PciPort => "PCI Port".to_string(),
            SerialPortType::Unknown => "Unknown Device".to_string(),
        };
        
        let manufacturer = match &port.port_type {
            SerialPortType::UsbPort(usb_info) => usb_info.manufacturer.clone(),
            _ => None,
        };
        
        discovered_ports.push(PortInfo {
            name: port.port_name,
            description,
            manufacturer,
        });
    }
    
    Ok(discovered_ports)
}

pub fn prompt_port_selection(ports: Vec<PortInfo>) -> Result<Option<String>> {
    println!("\nAvailable Serial Ports:");
    println!("========================");
    
    for (i, port) in ports.iter().enumerate() {
        println!("{}. {} - {}", i + 1, port.name, port.description);
        if let Some(manufacturer) = &port.manufacturer {
            println!("   Manufacturer: {}", manufacturer);
        }
    }
    
    println!("0. Cancel");
    println!();
    
    loop {
        print!("Select port (1-{}, 0 to cancel): ", ports.len());
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        match input.trim().parse::<usize>() {
            Ok(0) => {
                println!("Cancelled.");
                return Ok(None);
            }
            Ok(choice) if choice <= ports.len() => {
                let selected_port = &ports[choice - 1];
                println!("Selected: {} - {}", selected_port.name, selected_port.description);
                return Ok(Some(selected_port.name.clone()));
            }
            _ => {
                println!("Invalid selection. Please try again.");
            }
        }
    }
}