let logElement = document.getElementById('log');
let currentlyConnected = false;
let telescopeConnected = false;
let currentlySlewing = false;

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

// Toggle between Alpaca and Local connection fields
function toggleConnectionFields() {
    const connectionType = document.getElementById('connection-type').value;
    const alpacaFields = document.getElementById('alpaca-fields');
    const localFields = document.getElementById('local-fields');
    
    if (connectionType === 'alpaca') {
        alpacaFields.style.display = 'block';
        localFields.style.display = 'none';
    } else {
        alpacaFields.style.display = 'none';
        localFields.style.display = 'block';
        refreshTelescopeList();
    }
}

// Refresh list of local ASCOM telescopes
async function refreshTelescopeList() {
    try {
        const response = await fetch('/api/telescope/list');
        const data = await response.json();
        const select = document.getElementById('telescope-progid');
        
        select.innerHTML = '<option value="">Select a telescope driver...</option>';
        
        if (data.telescopes.length === 0) {
            select.innerHTML = '<option value="">No local ASCOM drivers found</option>';
            log('⚠️ No local ASCOM telescope drivers found');
        } else {
            data.telescopes.forEach(driver => {
                const option = document.createElement('option');
                option.value = driver;
                option.textContent = driver;
                select.appendChild(option);
            });
            log('🔄 Found ' + data.telescopes.length + ' local ASCOM telescope drivers');
        }
    } catch (error) {
        log('❌ Failed to list telescopes: ' + error.message);
    }
}

// Updated telescope connect function
async function connectTelescope() {
    const connectionType = document.getElementById('connection-type').value;
    let requestData = { connection_type: connectionType };
    
    if (connectionType === 'alpaca') {
        requestData.url = document.getElementById('telescope-url').value;
        requestData.device_number = parseInt(document.getElementById('telescope-device').value);
    } else {
        const progId = document.getElementById('telescope-progid').value;
        if (!progId) {
            log('❌ Please select a telescope driver');
            return;
        }
        requestData.prog_id = progId;
    }
    
    try {
        const response = await fetch('/api/telescope/connect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(requestData)
        });
        
        const data = await response.json();
        
        if (data.success) {
            log('🔭 ' + data.message);
            updateTelescopeButtons(true);
            // Load available axis rates
            loadAxisRates();
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

// Load available axis rates from telescope
async function loadAxisRates() {
    try {
        const response = await fetch('/api/telescope/axis_rates');
        const data = await response.json();
        
        if (data.rates && data.rates.length > 0) {
            const select = document.getElementById('slew-rate');
            select.innerHTML = '';
            
            data.rates.forEach((rate, index) => {
                const option = document.createElement('option');
                option.value = rate;
                option.textContent = rate + '°/s';
                if (index === 1) option.selected = true; // Select second rate by default
                select.appendChild(option);
            });
            
            log('📊 Loaded ' + data.rates.length + ' axis rates from telescope');
        }
    } catch (error) {
        log('⚠️ Using default axis rates: ' + error.message);
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

// Manual slew control functions
async function startManualSlew(direction) {
    if (currentlySlewing) return; // Prevent multiple simultaneous slews
    
    const rate = parseFloat(document.getElementById('slew-rate').value);
    
    try {
        const response = await fetch('/api/telescope/slew/manual', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ direction: direction, rate: rate })
        });
        
        const data = await response.json();
        
        if (data.success) {
            currentlySlewing = true;
            log('🎮 ' + data.message);
            // Update UI to show slewing
            const telescopeStatus = document.getElementById('telescope-status');
            telescopeStatus.className = 'status slewing';
            telescopeStatus.innerHTML = '🎯 Manual slewing ' + direction.toUpperCase();
        } else {
            log('❌ Manual slew failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to start manual slew: ' + error.message);
    }
}

async function stopManualSlew() {
    if (!currentlySlewing) return;
    
    try {
        const response = await fetch('/api/telescope/slew/stop', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });
        
        const data = await response.json();
        
        if (data.success) {
            currentlySlewing = false;
            log('⏹️ ' + data.message);
        } else {
            log('❌ Stop slew failed: ' + data.message);
        }
    } catch (error) {
        log('❌ Failed to stop slew: ' + error.message);
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

// Update button states to include manual slew controls
function updateTelescopeButtons(connected) {
    document.getElementById('telescope-connect-btn').disabled = connected;
    document.getElementById('telescope-disconnect-btn').disabled = !connected;
    
    // Connection type and fields
    document.getElementById('connection-type').disabled = connected;
    document.getElementById('telescope-url').disabled = connected;
    document.getElementById('telescope-device').disabled = connected;
    document.getElementById('telescope-progid').disabled = connected;
    
    // Enable/disable control buttons based on connection
    const controlButtons = ['slew-btn', 'abort-btn', 'tracking-btn', 'park-btn', 'unpark-btn', 'home-btn'];
    controlButtons.forEach(id => {
        document.getElementById(id).disabled = !connected;
    });
    
    // Enable/disable manual slew controls
    const dpadButtons = document.querySelectorAll('.dpad-btn');
    dpadButtons.forEach(btn => {
        btn.disabled = !connected;
    });
    
    document.getElementById('slew-rate').disabled = !connected;
    
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
        // Don't override manual slewing status
        if (!currentlySlewing) {
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
        }
        updateTelescopeButtons(true);
    } else {
        telescopeStatus.className = 'status disconnected';
        telescopeStatus.innerHTML = '❌ Telescope not connected';
        updateTelescopeButtons(false);
        currentlySlewing = false;
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

// Add touch support for mobile devices
document.addEventListener('DOMContentLoaded', function() {
    // Add touch event listeners for manual slew controls
    const dpadButtons = document.querySelectorAll('.dpad-btn:not(.dpad-stop)');
    
    dpadButtons.forEach(btn => {
        // Touch events for mobile
        btn.addEventListener('touchstart', function(e) {
            e.preventDefault();
            const direction = this.classList.contains('dpad-n') ? 'north' :
                           this.classList.contains('dpad-s') ? 'south' :
                           this.classList.contains('dpad-e') ? 'east' :
                           this.classList.contains('dpad-w') ? 'west' : null;
            if (direction) startManualSlew(direction);
        });
        
        btn.addEventListener('touchend', function(e) {
            e.preventDefault();
            stopManualSlew();
        });
    });
    
    // Initialize connection type
    toggleConnectionFields();
});

// Auto-refresh every 5 seconds
setInterval(fetchStatus, 5000);

// Initial load
log('🚀 Web interface v0.2.1 loaded');
fetchStatus();
refreshPorts();