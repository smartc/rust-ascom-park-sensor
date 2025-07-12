# nRF52840 Telescope Park Bridge v0.3.1

ASCOM Alpaca bridge for nRF52840 XIAO Sense based telescope park sensor with built-in LSM6DS3TR-C IMU.

## Features

- **Serial Communication**: Direct communication with nRF52840 XIAO Sense device
- **ASCOM Alpaca API**: Full compliance with ASCOM Safety Monitor specification
- **Web Interface**: Modern responsive web UI for device control and monitoring
- **Device Control**: Set park position, calibrate IMU, factory reset
- **Manual Commands**: Send raw commands to device and view responses
- **Auto-Discovery**: Automatically detects nRF52840-like devices

## Hardware Requirements

- **Primary**: Seeed Studio XIAO nRF52840 Sense with built-in LSM6DS3TR-C IMU
- **Alternative**: Any nRF52840-based device with compatible firmware

## Firmware Compatibility

This bridge is designed to work with the nRF52840 firmware that:
- Uses hex command protocol: `<XX>` format
- Returns JSON responses with `status`, `data`, `message` fields
- Supports commands: 01-0E (status, position, park control, calibration, etc.)

## Quick Start

### Installation

```bash
# Clone the repository
git clone <repository-url>
cd telescope_park_bridge

# Build the project
cargo build --release
```

### Usage

```bash
# Auto-detect nRF52840 device and start
./target/release/telescope_park_bridge --auto

# Specify a specific port
./target/release/telescope_park_bridge --port COM3  # Windows
./target/release/telescope_park_bridge --port /dev/ttyACM0  # Linux

# Enable debug logging
./target/release/telescope_park_bridge --debug --auto

# Custom bind address and port
./target/release/telescope_park_bridge --bind 0.0.0.0 --http-port 8080
```

### Web Interface

Once running, access the web interface at:
- **Local**: http://127.0.0.1:11111
- **Network**: http://YOUR_IP:11111

### ASCOM Alpaca Endpoints

The bridge provides standard ASCOM Alpaca endpoints:
- **Base URL**: http://127.0.0.1:11111/api/v1/safetymonitor/0/
- **Management**: http://127.0.0.1:11111/management/v1/

## Command Line Options

```
Options:
  -p, --port <PORT>          Serial port (e.g., COM3, /dev/ttyUSB0, /dev/ttyACM0)
  -b, --baud <BAUD>          Baud rate for serial communication [default: 115200]
      --bind <BIND>          HTTP server bind address [default: 127.0.0.1]
      --http-port <PORT>     HTTP server port for ASCOM Alpaca [default: 11111]
      --auto                 Auto-select first available nRF52840-like device
  -d, --debug                Enable debug logging
  -h, --help                 Print help
  -V, --version              Print version
```

## Device Commands

The nRF52840 firmware supports these hex commands:

| Command | Description |
|---------|-------------|
| `01` | Get device status |
| `02` | Get current position |
| `03` | Check if parked |
| `04` | Set park position |
| `05` | Get park position |
| `06` | Calibrate sensor |
| `07` | Toggle debug |
| `08` | Get version |
| `0A###` | Set tolerance (### = hundredths of degrees) |
| `0B` | Get tolerance |
| `0C` | Get system info |
| `0D` | Software set park |
| `0E` | Factory reset |

## Web Interface Features

### Park Sensor Tab
- Real-time connection status
- Device information display
- Current position and park status
- Serial port selection and connection

### Device Control Tab ⭐ NEW in v0.3.1
- **Set Park Position**: Set current position as park position
- **IMU Calibration**: Recalibrate the built-in sensor
- **Factory Reset**: Reset all settings to defaults
- **Manual Command Interface**: Send custom hex commands and view responses

### Activity Logs Tab
- Real-time activity logging
- ASCOM endpoint testing
- Connection status monitoring

## API Endpoints

### Web API
- `GET /api/status` - Get device state
- `GET /api/ports` - List available serial ports
- `POST /api/connect` - Connect to serial device
- `POST /api/disconnect` - Disconnect from device
- `POST /api/command` - Send manual command ⭐ NEW
- `POST /api/device/calibrate` - Calibrate IMU ⭐ NEW
- `POST /api/device/set_park` - Set park position ⭐ NEW
- `POST /api/device/factory_reset` - Factory reset ⭐ NEW

### ASCOM Alpaca API
- `GET /api/v1/safetymonitor/0/connected` - Connection status
- `GET /api/v1/safetymonitor/0/issafe` - Safety status (parked)
- `GET /api/v1/safetymonitor/0/name` - Device name
- `GET /api/v1/safetymonitor/0/description` - Device description
- `GET /management/v1/configureddevices` - Device list
- `GET /management/v1/description` - Server description

## Technical Details

### Serial Communication
- **Baud Rate**: 115200 (configurable)
- **Protocol**: Hex commands in `<XX>` format
- **Response**: JSON with status, data, message fields
- **Timeout**: 10 seconds for device responses

### Device State
The bridge maintains real-time state including:
- Connection status and error messages
- Device information (name, version, platform)
- Position data (pitch, roll, park position, tolerance)
- Park status and calibration state
- System information (uptime, capabilities)

### Error Handling
- Automatic reconnection on serial errors
- Timeout handling for device communication
- Graceful degradation when device unavailable
- Comprehensive error logging and user feedback

## Troubleshooting

### Device Not Detected
1. Check device is connected and powered
2. Verify correct drivers installed
3. Try different USB cable/port
4. Check device appears in system device manager
5. Use `--debug` flag for detailed logging

### Connection Issues
1. Ensure correct baud rate (115200)
2. Check no other software using the port
3. Verify device firmware is compatible
4. Try manual port selection instead of auto-detect

### ASCOM Issues
1. Test endpoints directly via web interface
2. Check Windows firewall settings
3. Verify ASCOM Platform installed (for local clients)
4. Use debug logging to trace API calls

## Development

### Building from Source
```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run with debug logging
cargo run -- --debug --auto
```

### Project Structure
```
src/
├── main.rs              # Application entry point
├── device_state.rs      # Device state management
├── serial_client.rs     # nRF52840 communication
├── alpaca_server.rs     # ASCOM Alpaca API server
├── port_discovery.rs    # Serial port detection
├── connection_manager.rs # Connection and command management ⭐ NEW
└── errors.rs           # Error types

templates/
├── index.html          # Web interface HTML
├── style.css           # Web interface styles
└── script.js           # Web interface JavaScript
```

## Changelog

### v0.3.1 ⭐ NEW FEATURES
- **Device Control Commands**: Set park position, calibrate IMU, factory reset
- **Manual Command Interface**: Send custom hex commands and view responses
- **Enhanced Connection Management**: Better command-response handling
- **Improved Error Handling**: More detailed error messages and logging
- **Better Web UI**: Enhanced device control tab with confirmation dialogs

### v0.3.0
- Complete rewrite for nRF52840 XIAO Sense compatibility
- Enhanced serial communication with proper JSON parsing
- Improved web interface with device control functions
- External template files for better maintainability
- Better error handling and device discovery
- Real-time status updates and activity logging

### v0.2.x (Abandoned)
- ESP32 compatibility (kept external templates concept)

### v0.1.0 (Original)
- Basic ESP32 functionality
- Embedded HTML templates

## License

[Specify your license here]

## Support

For issues and questions:
1. Check the troubleshooting section
2. Enable debug logging for detailed information
3. [Create an issue on GitHub]