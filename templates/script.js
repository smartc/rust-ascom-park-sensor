let logElement = document.getElementById('log');
let currentlyConnected = false;

function switchTab(tabName) {
    // Hide all tab contents
    const tabContents = document.querySelectorAll('.tab-content');
    tabContents.forEach(tab => tab.classList.remove('active'));
    
    // Remove active class from all tab buttons
    const tabButtons = document.querySelectorAll('.tab-button');
    tabButtons.forEach(button => button.classList.remove('active'));
    
    // Show selected tab content
    document.getElementById(tabName).classList.add('active');
    
    // Mark selected tab button as active
    event.target.classList.add('active');
    
    log('📑 Switched to ' + tabName + ' tab');
}

function log(message) {
    const timestamp = new Date().toLocaleTimeString();
    logElement.textContent += '[' + timestamp + '] ' + message + '\n';
    logElement.scrollTop = logElement.scrollHeight;
}

function clearLog() {
    logElement.textContent = '';
    log('🔄 Log cleared');
}

// Pretty print JSON with syntax highlighting
function formatJSON(jsonString) {
    try {
        const parsed = JSON.parse(jsonString);
        return JSON.stringify(parsed, null, 2);
    } catch (e) {
        // If it's not valid JSON, return as-is
        return jsonString;
    }
}

// Add syntax highlighting to JSON
function highlightJSON(jsonString) {
    return jsonString
        .replace(/("(\\u[a-zA-Z0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(true|false|null)\b|-?\d+(?:\.\d*)?(?:[eE][+\-]?\d+)?)/g, function (match) {
            let cls = 'number';
            if (/^"/.test(match)) {
                if (/:$/.test(match)) {
                    cls = 'key';
                } else {
                    cls = 'string';
                }
            } else if (/true|false/.test(match)) {
                cls = 'boolean';
            } else if (/null/.test(match)) {
                cls = 'null';
            }
            return '<span class="json-' + cls + '">' + match + '</span>';
        });
}

async function fetchStatus() {
    try {
        const response = await fetch('/api/status');
        const data = await response.json();
        updateUI(data);
    } catch (error) {
        log('❌ Failed to fetch status: ' + error.message);
    }
}

async function refreshPorts() {
    try {
        const response = await fetch('/api/ports');
        const data = await response.json();
        const select = document.getElementById('port-select');
        
        select.innerHTML = '<option value="">Select a port...</option>';
        
        if (data.ports.length === 0) {
            select.innerHTML = '<option value="">No serial ports found</option>';
            log('⚠️ No serial ports found');
        } else {
            data.ports.forEach(port => {
                const option = document.createElement('option');
                option.value = port.name;
                option.textContent = port.name + ' - ' + port.description;
                
                // Highlight likely nRF52840 devices
                if (port.description.toLowerCase().includes('xiao') || 
                    port.description.toLowerCase().includes('nrf52') ||
                    port.description.toLowerCase().includes('seeed')) {
                    option.textContent += ' ⭐';
                }
                
                select.appendChild(option);
            });
            log('🔄 Found ' + data.ports.length + ' available ports');
        }
    } catch (error) {
        log('❌ Failed to refresh ports: ' + error.message);
    }
}

// Serial connection functions
async function connectToPort() {
    const port = document.getElementById('port-select').value;
    const baudRate = parseInt(document.getElementById('baud-rate').value);
    
    if (!port) {
        log('❌ Please select a port first');
        return;
    }
    
    try {
        log('🔌 Connecting to nRF52840 device on ' + port + '...');
        
        const response = await fetch('/api/connect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ port: port, baud_rate: baudRate })
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('✅ ' + data.message);
            updateConnectionButtons(true);
        } else {
            log('❌ Connection failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to connect: ' + error.message);
    }
}

async function disconnectFromPort() {
    try {
        log('🔌 Disconnecting from nRF52840 device...');
        
        const response = await fetch('/api/disconnect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('✅ ' + data.message);
            updateConnectionButtons(false);
        } else {
            log('❌ Disconnect failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to disconnect: ' + error.message);
    }
}

// Device control functions
async function setParkPosition() {
    if (!currentlyConnected) {
        log('❌ Device not connected');
        return;
    }
    
    if (!confirm('Set the current telescope position as the park position?')) {
        return;
    }
    
    try {
        log('📍 Setting current position as park position...');
        
        const response = await fetch('/api/device/set_park', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('✅ ' + data.message);
        } else {
            log('❌ Failed to set park position: ' + data.message);
        }
    } catch (error) {
        log('❌ Error setting park position: ' + error.message);
    }
}

async function calibrateSensor() {
    if (!currentlyConnected) {
        log('❌ Device not connected');
        return;
    }
    
    if (!confirm('Calibrate the IMU sensor? Keep the device still during calibration.')) {
        return;
    }
    
    try {
        log('🎯 Starting IMU calibration...');
        
        const response = await fetch('/api/device/calibrate', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('✅ ' + data.message);
        } else {
            log('❌ Calibration failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Error during calibration: ' + error.message);
    }
}

async function factoryReset() {
    if (!currentlyConnected) {
        log('❌ Device not connected');
        return;
    }
    
    if (!confirm('⚠️ FACTORY RESET ⚠️\n\nThis will erase ALL settings including:\n- Park position\n- Calibration data\n- Tolerance settings\n\nAre you sure you want to continue?')) {
        return;
    }
    
    if (!confirm('This action CANNOT be undone. Are you absolutely sure?')) {
        return;
    }
    
    try {
        log('🏭 Performing factory reset...');
        
        const response = await fetch('/api/device/factory_reset', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('✅ ' + data.message);
        } else {
            log('❌ Factory reset failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Error during factory reset: ' + error.message);
    }
}

async function sendManualCommand() {
    const command = document.getElementById('manual-command').value.trim().toUpperCase();
    
    if (!currentlyConnected) {
        log('❌ Device not connected');
        return;
    }
    
    if (!command) {
        log('❌ Please enter a command');
        return;
    }
    
    // Validate command format (hex digits only)
    if (!/^[0-9A-F]+$/.test(command)) {
        log('❌ Invalid command format. Use hex digits only (e.g., 01, 02, 0A050)');
        return;
    }
    
    try {
        log('📤 Sending command: <' + command + '>');
        
        const response = await fetch('/api/command', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ command: command })
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('✅ Command sent successfully');
            if (data.response) {
                document.getElementById('command-response').style.display = 'block';
                
                // Pretty print and highlight the JSON response
                const formattedJSON = formatJSON(data.response);
                const highlightedJSON = highlightJSON(formattedJSON);
                document.getElementById('response-text').innerHTML = highlightedJSON;
                
                log('📥 Response received (see formatted output below)');
            }
        } else {
            log('❌ Command failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Error sending command: ' + error.message);
    }
    
    // Clear the command input
    document.getElementById('manual-command').value = '';
}

function updateConnectionButtons(connected) {
    const connectBtn = document.getElementById('connect-btn');
    const disconnectBtn = document.getElementById('disconnect-btn');
    const portSelect = document.getElementById('port-select');
    const baudRate = document.getElementById('baud-rate');
    
    // Connection buttons
    connectBtn.disabled = connected;
    disconnectBtn.disabled = !connected;
    portSelect.disabled = connected;
    baudRate.disabled = connected;
    
    // Device control buttons
    document.getElementById('set-park-btn').disabled = !connected;
    document.getElementById('calibrate-btn').disabled = !connected;
    document.getElementById('factory-reset-btn').disabled = !connected;
    document.getElementById('send-command-btn').disabled = !connected;
    
    currentlyConnected = connected;
}

function updateUI(data) {
    // Header park status (visible on all tabs)
    const headerStatus = document.getElementById('header-park-status');
    if (data.connected) {
        if (data.is_parked || data.is_safe) {
            headerStatus.className = 'header-status parked';
            headerStatus.innerHTML = '✅ TELESCOPE PARKED';
        } else {
            headerStatus.className = 'header-status not-parked';
            headerStatus.innerHTML = '⚠️ NOT PARKED';
        }
    } else {
        headerStatus.className = 'header-status disconnected';
        headerStatus.innerHTML = '🚫 DISCONNECTED';
    }

    // Connection status
    const connStatus = document.getElementById('connection-status');
    if (data.connected) {
        connStatus.className = 'status connected';
        connStatus.innerHTML = '✅ Connected to nRF52840 device';
        updateConnectionButtons(true);
    } else {
        connStatus.className = 'status disconnected';
        connStatus.innerHTML = '❌ Not connected to nRF52840 device';
        if (data.error_message) {
            connStatus.innerHTML += ' - ' + data.error_message;
        }
        updateConnectionButtons(false);
    }
    
    // Safety status (park status)
    const safetyStatus = document.getElementById('safety-status');
    if (data.connected) {
        if (data.is_parked || data.is_safe) {
            safetyStatus.className = 'status safe';
            safetyStatus.innerHTML = '✅ Telescope is PARKED (Safe)';
        } else {
            safetyStatus.className = 'status unsafe';
            safetyStatus.innerHTML = '⚠️ Telescope is NOT PARKED (Unsafe)';
        }
    } else {
        safetyStatus.className = 'status unsafe';
        safetyStatus.innerHTML = '🚫 Safety status unknown (disconnected)';
    }
    
    // Device information - clear when disconnected
    if (data.connected) {
        document.getElementById('device-name').textContent = data.device_name;
        document.getElementById('device-version').textContent = data.device_version;
        document.getElementById('manufacturer').textContent = data.manufacturer;
        document.getElementById('platform').textContent = data.platform;
        document.getElementById('imu').textContent = data.imu;
        document.getElementById('serial-port').textContent = data.serial_port || 'Not connected';
    } else {
        // Clear all device information when disconnected
        document.getElementById('device-name').textContent = 'Not connected';
        document.getElementById('device-version').textContent = '--';
        document.getElementById('manufacturer').textContent = '--';
        document.getElementById('platform').textContent = '--';
        document.getElementById('imu').textContent = '--';
        document.getElementById('serial-port').textContent = 'Not connected';
    }
    
    // Position data - clear when disconnected
    if (data.connected) {
        document.getElementById('current-pitch').textContent = data.current_pitch.toFixed(2);
        document.getElementById('current-roll').textContent = data.current_roll.toFixed(2);
        document.getElementById('park-pitch').textContent = data.park_pitch.toFixed(2);
        document.getElementById('park-roll').textContent = data.park_roll.toFixed(2);
        document.getElementById('tolerance').textContent = data.position_tolerance.toFixed(1);
        document.getElementById('calibrated').textContent = data.is_calibrated ? 'Yes' : 'No';
    } else {
        // Clear all position data when disconnected
        document.getElementById('current-pitch').textContent = '--';
        document.getElementById('current-roll').textContent = '--';
        document.getElementById('park-pitch').textContent = '--';
        document.getElementById('park-roll').textContent = '--';
        document.getElementById('tolerance').textContent = '--';
        document.getElementById('calibrated').textContent = '--';
    }
}

function refreshStatus() {
    log('🔄 Refreshing status...');
    fetchStatus();
}

async function testASCOMConnection() {
    log('🧪 Testing ASCOM Alpaca connection...');
    try {
        const response = await fetch('/api/v1/safetymonitor/0/connected');
        const data = await response.json();
        if (data.ErrorNumber === 0) {
            log('✅ ASCOM test successful - Connected: ' + data.Value);
        } else {
            log('❌ ASCOM test failed - Error: ' + data.ErrorMessage);
        }
        
        // Test safety status as well
        const safetyResponse = await fetch('/api/v1/safetymonitor/0/issafe');
        const safetyData = await safetyResponse.json();
        if (safetyData.ErrorNumber === 0) {
            log('✅ ASCOM safety test - Is Safe: ' + safetyData.Value);
        } else {
            log('❌ ASCOM safety test failed - Error: ' + safetyData.ErrorMessage);
        }
    } catch (error) {
        log('❌ ASCOM test failed: ' + error.message);
    }
}

// Handle Enter key in manual command input
document.addEventListener('DOMContentLoaded', function() {
    const commandInput = document.getElementById('manual-command');
    if (commandInput) {
        commandInput.addEventListener('keypress', function(e) {
            if (e.key === 'Enter') {
                sendManualCommand();
            }
        });
    }
});

// Auto-refresh every 1 second for real-time updates
setInterval(fetchStatus, 1000);

// Initial load
log('🚀 nRF52840 Telescope Park Bridge v0.3.1 loaded');
log('🔧 Target device: XIAO Sense with LSM6DS3TR-C IMU');
log('⚡ Real-time updates: 1 second refresh rate');
log('🎛️ Device control features: Set Park, Calibrate, Factory Reset, Manual Commands');
fetchStatus();
refreshPorts();