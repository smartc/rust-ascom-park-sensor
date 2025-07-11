use anyhow::Result;
use serialport::SerialPortType;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PortInfo {
    pub name: String,
    pub description: String,
    pub manufacturer: Option<String>,
    pub vid_pid: Option<String>,
}

pub fn discover_ports() -> Result<Vec<PortInfo>> {
    let ports = serialport::available_ports()?;
    
    let mut discovered_ports = Vec::new();
    
    for port in ports {
        let (description, manufacturer, vid_pid) = match &port.port_type {
            SerialPortType::UsbPort(usb_info) => {
                let vid_pid = format!("VID:{:04X} PID:{:04X}", usb_info.vid, usb_info.pid);
                
                // Enhanced description for known nRF52840 devices
                let description = if usb_info.vid == 0x2886 {  // Seeed Studio VID
                    "Seeed Studio XIAO nRF52840 (or compatible)".to_string()
                } else if usb_info.vid == 0x239A {  // Adafruit VID (sometimes used for nRF52840)
                    "Adafruit/Nordic nRF52840 (or compatible)".to_string()
                } else if usb_info.vid == 0x1915 && usb_info.pid == 0x521F {  // Nordic VID/PID
                    "Nordic nRF52840 Development Kit".to_string()
                } else if usb_info.vid == 0x1A86 {  // CH340 chip (common on Chinese boards)
                    format!("USB Serial Device (CH340) - {}", vid_pid)
                } else if usb_info.vid == 0x10C4 {  // CP210x chip
                    format!("USB Serial Device (CP210x) - {}", vid_pid)
                } else if usb_info.vid == 0x0403 {  // FTDI chip
                    format!("USB Serial Device (FTDI) - {}", vid_pid)
                } else {
                    format!("USB Serial Device - {}", vid_pid)
                };
                
                (description, usb_info.manufacturer.clone(), Some(vid_pid))
            }
            SerialPortType::BluetoothPort => {
                ("Bluetooth Serial Port".to_string(), None, None)
            }
            SerialPortType::PciPort => {
                ("PCI Serial Port".to_string(), None, None)
            }
            SerialPortType::Unknown => {
                ("Unknown Serial Device".to_string(), None, None)
            }
        };
        
        discovered_ports.push(PortInfo {
            name: port.port_name,
            description,
            manufacturer,
            vid_pid,
        });
    }
    
    // Sort ports to prioritize likely nRF52840 devices
    discovered_ports.sort_by(|a, b| {
        let a_priority = get_device_priority(&a.description);
        let b_priority = get_device_priority(&b.description);
        b_priority.cmp(&a_priority) // Higher priority first
    });
    
    Ok(discovered_ports)
}

fn get_device_priority(description: &str) -> i32 {
    let desc_lower = description.to_lowercase();
    
    // Higher numbers = higher priority
    if desc_lower.contains("xiao") || desc_lower.contains("nrf52840") {
        100  // Highest priority for nRF52840 devices
    } else if desc_lower.contains("seeed") || desc_lower.contains("nordic") {
        90   // High priority for Nordic/Seeed devices
    } else if desc_lower.contains("adafruit") {
        80   // Medium-high priority for Adafruit devices
    } else if desc_lower.contains("ch340") || desc_lower.contains("cp210") {
        50   // Medium priority for common USB-serial chips
    } else if desc_lower.contains("ftdi") {
        40   // Lower priority for FTDI devices
    } else if desc_lower.contains("bluetooth") {
        10   // Low priority for Bluetooth
    } else {
        0    // Lowest priority for unknown devices
    }
}