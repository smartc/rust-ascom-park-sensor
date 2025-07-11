let logElement = document.getElementById('log');
let currentlyConnected = false;
let telescopeConnected = false;

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
        
        data.ports.forEach(port => {
            const option = document.createElement('option');
            option.value = port.name;
            option.textContent = port.name + ' - ' + port.description;
            select.appendChild(option);
        });
        
        log('🔄 Refreshed ' + data.ports.length + ' available ports');
    } catch (error) {
        log('❌ Failed to refresh ports: ' + error.message);
    }
}

// Park sensor functions
async function connectToPort() {
    const port = document.getElementById('port-select').value;
    const baudRate = parseInt(document.getElementById('baud-rate').value);
    
    if (!port) {
        log('❌ Please select a port first');
        return;
    }
    
    try {
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

// Telescope functions
async function connectTelescope() {
    const url = document.getElementById('telescope-url').value;
    const deviceNumber = parseInt(document.getElementById('telescope-device').value);
    
    try {
        const response = await fetch('/api/telescope/connect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ url: url, device_number: deviceNumber })
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🔭 ' + data.message);
            updateTelescopeButtons(true);
        } else {
            log('❌ Telescope connection failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to connect to telescope: ' + error.message);
    }
}

async function disconnectTelescope() {
    try {
        const response = await fetch('/api/telescope/disconnect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🔭 ' + data.message);
            updateTelescopeButtons(false);
        } else {
            log('❌ Telescope disconnect failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to disconnect telescope: ' + error.message);
    }
}

async function slewToCoordinates() {
    const ra = parseFloat(document.getElementById('slew-ra').value);
    const dec = parseFloat(document.getElementById('slew-dec').value);
    
    if (isNaN(ra) || isNaN(dec)) {
        log('❌ Please enter valid RA and Dec coordinates');
        return;
    }
    
    try {
        const response = await fetch('/api/telescope/slew', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ra: ra, dec: dec })
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🎯 Slewing to RA: ' + ra + 'h, Dec: ' + dec + '°');
        } else {
            log('❌ Slew failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to slew telescope: ' + error.message);
    }
}

async function abortSlew() {
    try {
        const response = await fetch('/api/telescope/abort', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('⏹️ Telescope slew aborted');
        } else {
            log('❌ Abort failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to abort slew: ' + error.message);
    }
}

async function toggleTracking() {
    try {
        const response = await fetch('/api/telescope/tracking', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🎯 Tracking toggled');
        } else {
            log('❌ Tracking toggle failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to toggle tracking: ' + error.message);
    }
}

async function parkTelescope() {
    try {
        const response = await fetch('/api/telescope/park', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🏠 Telescope parking');
        } else {
            log('❌ Park failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to park telescope: ' + error.message);
    }
}

async function unparkTelescope() {
    try {
        const response = await fetch('/api/telescope/unpark', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🚀 Telescope unparking');
        } else {
            log('❌ Unpark failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to unpark telescope: ' + error.message);
    }
}

async function findHome() {
    try {
        const response = await fetch('/api/telescope/home', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🏁 Telescope finding home');
        } else {
            log('❌ Find home failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to find home: ' + error.message);
    }
}

function updateConnectionButtons(connected) {
    const connectBtn = document.getElementById('connect-btn');
    const disconnectBtn = document.getElementById('disconnect-btn');
    const portSelect = document.getElementById('port-select');
    const baudRate = document.getElementById('baud-rate');
    
    connectBtn.disabled = connected;
    disconnectBtn.disabled = !connected;
    portSelect.disabled = connected;
    baudRate.disabled = connected;
    
    currentlyConnected = connected;
}

function updateTelescopeButtons(connected) {
    document.getElementById('telescope-connect-btn').disabled = connected;
    document.getElementById('telescope-disconnect-btn').disabled = !connected;
    
    // Enable/disable control buttons based on connection
    const controlButtons = ['slew-btn', 'abort-btn', 'tracking-btn', 'park-btn', 'unpark-btn', 'home-btn'];
    controlButtons.forEach(id => {
        document.getElementById(id).disabled = !connected;
    });
    
    telescopeConnected = connected;
}

function updateUI(data) {
    // Park sensor status
    const connStatus = document.getElementById('connection-status');
    if (data.connected) {
        connStatus.className = 'status connected';
        connStatus.innerHTML = '✅ Connected to park sensor';
        updateConnectionButtons(true);
    } else {
        connStatus.className = 'status disconnected';
        connStatus.innerHTML = '❌ Not connected to park sensor';
        if (data.error_message) {
            connStatus.innerHTML += ' - ' + data.error_message;
        }
        updateConnectionButtons(false);
    }
    
    // Safety status
    const safetyStatus = document.getElementById('safety-status');
    if (data.connected) {
        if (data.is_safe) {
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
    
    // Telescope status
    const telescopeStatus = document.getElementById('telescope-status');
    if (data.telescope_connected) {
        if (data.telescope_status.slewing) {
            telescopeStatus.className = 'status slewing';
            telescopeStatus.innerHTML = '🎯 Telescope is SLEWING';
        } else if (data.telescope_status.at_park) {
            telescopeStatus.className = 'status safe';
            telescopeStatus.innerHTML = '🏠 Telescope is PARKED';
        } else if (data.telescope_status.tracking) {
            telescopeStatus.className = 'status connected';
            telescopeStatus.innerHTML = '🎯 Telescope is TRACKING';
        } else {
            telescopeStatus.className = 'status warning';
            telescopeStatus.innerHTML = '⚠️ Telescope connected but not tracking';
        }
        updateTelescopeButtons(true);
    } else {
        telescopeStatus.className = 'status disconnected';
        telescopeStatus.innerHTML = '❌ Telescope not connected';
        updateTelescopeButtons(false);
    }
    
    // Device info
    document.getElementById('device-name').textContent = data.device_name;
    document.getElementById('device-version').textContent = data.device_version;
    document.getElementById('manufacturer').textContent = data.manufacturer;
    document.getElementById('serial-port').textContent = data.serial_port || 'Not connected';
    
    // Position data
    document.getElementById('current-pitch').textContent = data.current_pitch.toFixed(2);
    document.getElementById('current-roll').textContent = data.current_roll.toFixed(2);
    document.getElementById('park-pitch').textContent = data.park_pitch.toFixed(2);
    document.getElementById('park-roll').textContent = data.park_roll.toFixed(2);
    document.getElementById('tolerance').textContent = data.position_tolerance.toFixed(1);
    
    // Telescope data
    if (data.telescope_connected) {
        document.getElementById('telescope-name').textContent = data.telescope_status.name;
        document.getElementById('telescope-tracking').textContent = data.telescope_status.tracking ? 'Yes' : 'No';
        document.getElementById('telescope-slewing').textContent = data.telescope_status.slewing ? 'Yes' : 'No';
        document.getElementById('telescope-at-park').textContent = data.telescope_status.at_park ? 'Yes' : 'No';
        document.getElementById('telescope-at-home').textContent = data.telescope_status.at_home ? 'Yes' : 'No';
        document.getElementById('telescope-pier-side').textContent = data.telescope_status.pier_side;
        document.getElementById('telescope-ra').textContent = data.telescope_status.ra.toFixed(3);
        document.getElementById('telescope-dec').textContent = data.telescope_status.dec.toFixed(3);
        document.getElementById('telescope-azimuth').textContent = data.telescope_status.azimuth.toFixed(1);
        document.getElementById('telescope-altitude').textContent = data.telescope_status.altitude.toFixed(1);
    } else {
        const telescopeFields = ['telescope-name', 'telescope-tracking', 'telescope-slewing', 
                               'telescope-at-park', 'telescope-at-home', 'telescope-pier-side',
                               'telescope-ra', 'telescope-dec', 'telescope-azimuth', 'telescope-altitude'];
        telescopeFields.forEach(id => {
            document.getElementById(id).textContent = '--';
        });
    }
}

function refreshStatus() {
    log('🔄 Refreshing status...');
    fetchStatus();
}

async function testConnection() {
    log('🧪 Testing ASCOM connection...');
    try {
        const response = await fetch('/api/v1/safetymonitor/0/connected');
        const data = await response.json();
        if (data.ErrorNumber === 0) {
            log('✅ ASCOM test successful - Connected: ' + data.Value);
        } else {
            log('❌ ASCOM test failed - Error: ' + data.ErrorMessage);
        }
    } catch (error) {
        log('❌ ASCOM test failed: ' + error.message);
    }
}

// Auto-refresh every 5 seconds
setInterval(fetchStatus, 5000);

// Initial load
log('🚀 Web interface v2.0 loaded');
fetchStatus();
refreshPorts();