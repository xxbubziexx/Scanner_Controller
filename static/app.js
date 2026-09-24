let socket = null;
let currentAppState = null;
let pollingInterval = null;
let modelsCatalog = [];
let availableComPorts = [];

function loadAvailableComPorts(callback) {
  fetch('/api/ports/available')
    .then(res => res.json())
    .then(data => {
      if (data.status === "success") {
        availableComPorts = data.ports;
        populateComPortDropdowns();
        if (callback) callback();
      }
    })
    .catch(err => console.error("Failed to load available COM ports:", err));
}

function populateComPortDropdowns() {
  const selA = document.getElementById("cfg_port_a");
  const selB = document.getElementById("cfg_port_b");
  if (!selA || !selB) return;

  function buildOptions(currentVal) {
    let html = "";
    if (availableComPorts.length === 0) {
      html += `<option value="NONE">None Detected (Click Refresh)</option>`;
    } else {
      availableComPorts.forEach(p => {
        const selected = (p.device === currentVal) ? "selected" : "";
        html += `<option value="${p.device}" ${selected}>${p.label}</option>`;
      });
    }
    // Also include currently configured port if it wasn't enumerated
    if (currentVal && !availableComPorts.some(p => p.device === currentVal) && currentVal !== "NONE") {
      html = `<option value="${currentVal}" selected>${currentVal} (Configured)</option>` + html;
    }
    html += `<option value="CUSTOM">Manual / Custom Port...</option>`;
    return html;
  }

  const curA = currentAppState && currentAppState.config ? currentAppState.config.scanner_a.port : "COM3";
  const curB = currentAppState && currentAppState.config ? currentAppState.config.scanner_b.port : "COM4";

  selA.innerHTML = buildOptions(curA);
  selB.innerHTML = buildOptions(curB);

  selA.onchange = () => handlePortSelectChange('a');
  selB.onchange = () => handlePortSelectChange('b');
}

function handlePortSelectChange(suffix) {
  const sel = document.getElementById(`cfg_port_${suffix}`);
  const customInput = document.getElementById(`cfg_custom_port_${suffix}`);
  if (!sel || !customInput) return;

  if (sel.value === "CUSTOM") {
    customInput.style.display = "block";
    customInput.focus();
  } else {
    customInput.style.display = "none";
  }
}

function toggleConnectionModeUI(suffix) {
  const modeSel = document.getElementById(`cfg_mode_${suffix}`);
  const portSel = document.getElementById(`cfg_port_${suffix}`);
  const customInput = document.getElementById(`cfg_custom_port_${suffix}`);
  const lbl = document.getElementById(`lbl_port_${suffix}`);
  if (!modeSel) return;

  if (modeSel.value === "IP") {
    if (lbl) lbl.innerText = "IP Address / Hostname";
    if (portSel) portSel.style.display = "none";
    if (customInput) {
      customInput.style.display = "block";
      customInput.placeholder = "e.g. 192.168.1.50:5000";
    }
  } else {
    if (lbl) lbl.innerText = "Available COM Port";
    if (portSel) portSel.style.display = "block";
    handlePortSelectChange(suffix);
  }
}

function refreshComPorts() {
  loadAvailableComPorts(() => {
    alert(`Refreshed available COM ports!\nFound ${availableComPorts.length} port(s):\n` + 
          availableComPorts.map(p => `• ${p.device} (${p.description})`).join('\n'));
  });
}

function connectWebSocket() {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${protocol}//${window.location.host}/ws`;

  try {
    socket = new WebSocket(wsUrl);

    socket.onopen = function () {
      console.log("WebSocket connected to Uniden Sync Server");
      stopPollingFallback();
    };

    socket.onmessage = function (event) {
      const data = JSON.parse(event.data);
      currentAppState = data;
      renderUI(data);
    };

    socket.onclose = function () {
      console.warn("WebSocket disconnected. Falling back to HTTP REST polling...");
      startPollingFallback();
      setTimeout(connectWebSocket, 3000);
    };

    socket.onerror = function (err) {
      console.error("WebSocket error:", err);
      startPollingFallback();
    };
  } catch (e) {
    console.error("Failed to establish WebSocket:", e);
    startPollingFallback();
  }
}

function startPollingFallback() {
  if (!pollingInterval) {
    pollingInterval = setInterval(fetchStateViaHTTP, 300);
  }
}

function stopPollingFallback() {
  if (pollingInterval) {
    clearInterval(pollingInterval);
    pollingInterval = null;
  }
}

function fetchStateViaHTTP() {
  fetch('/api/state')
    .then(res => res.json())
    .then(data => {
      currentAppState = data;
      renderUI(data);
    })
    .catch(err => {
      console.warn("HTTP state fetch failed:", err);
    });
}

function loadModelsCatalog() {
  fetch('/api/models/catalog')
    .then(res => res.json())
    .then(data => {
      if (data.status === "success") {
        modelsCatalog = data.models;
        populateModelDropdowns();
      }
    });
}

function populateModelDropdowns() {
  const selA = document.getElementById("cfg_model_a");
  const selB = document.getElementById("cfg_model_b");
  const cardSelA = document.getElementById("cardModelSelectA");
  const cardSelB = document.getElementById("cardModelSelectB");

  if (modelsCatalog.length === 0) return;

  let optsHtml = "";
  modelsCatalog.forEach(m => {
    optsHtml += `<option value="${m.model}">${m.model}</option>`;
  });

  if (selA) selA.innerHTML = optsHtml;
  if (selB) selB.innerHTML = optsHtml;
  if (cardSelA) cardSelA.innerHTML = optsHtml;
  if (cardSelB) cardSelB.innerHTML = optsHtml;

  if (currentAppState && currentAppState.config) {
    if (selA) selA.value = currentAppState.config.scanner_a.model;
    if (selB) selB.value = currentAppState.config.scanner_b.model;
    if (cardSelA) cardSelA.value = currentAppState.config.scanner_a.model;
    if (cardSelB) cardSelB.value = currentAppState.config.scanner_b.model;
  }

  updateBaudOptions('a');
  updateBaudOptions('b');
}

function changeModelFromCard(scannerId, newModel) {
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({
      type: "CHANGE_MODEL",
      scanner_id: scannerId,
      model: newModel
    }));
  } else {
    fetch(`/api/scanner/${scannerId}/model`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ model: newModel })
    }).catch(err => console.error("Error switching model:", err));
  }
}

function updateBaudOptions(suffix) {
  const modelSel = document.getElementById(`cfg_model_${suffix}`);
  const baudSel = document.getElementById(`cfg_baud_${suffix}`);
  if (!modelSel || !baudSel) return;

  const selectedModel = modelSel.value;
  const match = modelsCatalog.find(m => m.model === selectedModel);
  const allowed = match ? match.allowed_baud_rates : [115200, 57600, 38400, 19200, 9600, 4800];

  let baudHtml = "";
  allowed.forEach(b => {
    baudHtml += `<option value="${b}">${b}</option>`;
  });
  baudSel.innerHTML = baudHtml;

  if (currentAppState && currentAppState.config) {
    const currentBaud = suffix === 'a' ? currentAppState.config.scanner_a.baud_rate : currentAppState.config.scanner_b.baud_rate;
    if (allowed.includes(currentBaud)) {
      baudSel.value = currentBaud;
    }
  }
}

function renderUI(state) {
  if (!state) return;

  const scA = state.scanner_a || {};
  const scB = state.scanner_b || {};
  const cfg = state.config || {};
  const events = state.event_log || [];

  // Update Model Selects / Badges
  const cardSelA = document.getElementById("cardModelSelectA");
  const cardSelB = document.getElementById("cardModelSelectB");
  if (cardSelA && scA.model && document.activeElement !== cardSelA) {
    cardSelA.value = scA.model;
  }
  if (cardSelB && scB.model && document.activeElement !== cardSelB) {
    cardSelB.value = scB.model;
  }

  const badgeA = document.getElementById("modelBadgeA");
  const badgeB = document.getElementById("modelBadgeB");
  if (badgeA && scA.model) badgeA.innerText = scA.model;
  if (badgeB && scB.model) badgeB.innerText = scB.model;

  // 1. Master Sync Toggle
  const syncToggle = document.getElementById("masterSyncToggle");
  if (syncToggle && cfg.sync) {
    syncToggle.checked = cfg.sync.enable_sync;
  }

  // 2. Rules Controls
  if (cfg.sync) {
    document.getElementById("exclusionModeSelect").value = cfg.sync.exclusion_mode;
    document.getElementById("priorityModeSelect").value = cfg.sync.priority_mode;
    document.getElementById("primaryScannerSelect").value = cfg.sync.primary_scanner_id;
  }

  // 3. Engine Status Banner
  const statusBanner = document.getElementById("engineModeStatus");
  if (statusBanner && cfg.sync) {
    statusBanner.innerHTML = `● Engine Active <span style="color: var(--emerald-recv);">(Hardware Serial Mode)</span>`;
  }

  // 4. Render Scanner A Card
  renderScannerCard("A", scA);

  // 5. Render Scanner B Card
  renderScannerCard("B", scB);

  // 6. Render Event Log Table
  renderLogTable(events);
}

function renderScannerCard(suffix, data) {
  const card = document.getElementById(`card${suffix}`);
  const badge = document.getElementById(`badge${suffix}`);
  const statusText = document.getElementById(`statusText${suffix}`);

  const sysEl = document.getElementById(`sys${suffix}`);
  const deptEl = document.getElementById(`dept${suffix}`);
  const chanEl = document.getElementById(`chan${suffix}`);
  const tgidEl = document.getElementById(`tgid${suffix}`);
  const freqEl = document.getElementById(`freq${suffix}`);
  const modEl = document.getElementById(`mod${suffix}`);
  const rssiContainer = document.getElementById(`rssi${suffix}`);

  if (!card) return;

  sysEl.innerText = data.system_name || "Scanning...";
  deptEl.innerText = data.dept_name || "";
  chanEl.innerText = data.channel_name || "Scanning...";
  tgidEl.innerText = data.tgid || "--";
  freqEl.innerText = data.frequency ? `${data.frequency.toFixed(4)} MHz` : "-- MHz";
  modEl.innerText = data.modulation || "NFM";

  // State Badges
  if (data.receiving) {
    card.classList.add("receiving");
    badge.className = "status-badge receiving";
    statusText.innerText = "RECEIVING AUDIO";
  } else if (data.avoided) {
    card.classList.remove("receiving");
    badge.className = "status-badge avoided";
    statusText.innerText = "MUTUAL EXCLUDED";
  } else if (data.status_line && data.status_line.startsWith("HOLD")) {
    card.classList.remove("receiving");
    badge.className = "status-badge holding";
    statusText.innerText = "CHANNEL HOLD";
  } else if (data.status_line && data.status_line.includes("NO PORT")) {
    card.classList.remove("receiving");
    badge.className = "status-badge scanning";
    statusText.innerText = "OFFLINE (NO PORT)";
  } else if (!data.connected) {
    card.classList.remove("receiving");
    badge.className = "status-badge scanning";
    statusText.innerText = "DISCONNECTED";
  } else {
    card.classList.remove("receiving");
    badge.className = "status-badge scanning";
    statusText.innerText = "SCANNING";
  }

  // RSSI Bars
  if (rssiContainer) {
    const bars = rssiContainer.querySelectorAll(".rssi-bar");
    const level = data.rssi || (data.receiving ? 4 : 0);
    bars.forEach((bar, idx) => {
      if (idx < level) {
        bar.classList.add("active");
      } else {
        bar.classList.remove("active");
      }
    });
  }
}

function renderLogTable(events) {
  const body = document.getElementById("logTableBody");
  const countEl = document.getElementById("logCount");

  if (!body) return;

  if (!events || events.length === 0) {
    body.innerHTML = `<tr><td colspan="6" style="text-align: center; color: var(--text-dim); padding: 20px;">No mutual exclusion events recorded yet.</td></tr>`;
    if (countEl) countEl.innerText = "0 events recorded";
    return;
  }

  if (countEl) countEl.innerText = `${events.length} events recorded`;

  let html = "";
  events.forEach(e => {
    html += `
      <tr>
        <td class="log-time">${e.time_str}</td>
        <td><strong style="color: var(--cyan-accent);">${e.primary_id.toUpperCase()}</strong></td>
        <td><strong style="color: var(--violet-accent);">${e.target_id.toUpperCase()}</strong></td>
        <td><code style="color: #ffffff;">${e.tgid || e.freq}</code></td>
        <td>${e.channel}</td>
        <td class="log-reason">${e.reason}</td>
      </tr>
    `;
  });

  body.innerHTML = html;
}

function openAutoDetectModal() {
  const modal = document.getElementById("autoDetectModal");
  if (modal) modal.classList.add("active");
  runAutoDetect();
}

function closeAutoDetectModal() {
  const modal = document.getElementById("autoDetectModal");
  if (modal) modal.classList.remove("active");
}

function runAutoDetect() {
  const body = document.getElementById("autoDetectTableBody");
  const btn = document.getElementById("btnRunAutoDetect");
  if (btn) btn.innerText = "⏳ Probing Ports...";

  if (body) {
    body.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-dim); padding: 20px;">Probing COM ports at standard baud rates... Please wait.</td></tr>`;
  }

  fetch('/api/ports/detect')
    .then(res => res.json())
    .then(data => {
      if (btn) btn.innerText = "🔄 Run Serial Auto-Detect";
      if (data.status === "success") {
        renderAutoDetectGrid(data.ports);
      }
    })
    .catch(err => {
      if (btn) btn.innerText = "🔄 Run Serial Auto-Detect";
      if (body) body.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--rose-danger); padding: 20px;">Failed to detect ports: ${err}</td></tr>`;
    });
}

function renderAutoDetectGrid(ports) {
  const body = document.getElementById("autoDetectTableBody");
  if (!body) return;

  if (!ports || ports.length === 0) {
    body.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-dim); padding: 20px;">No serial COM ports found on system.</td></tr>`;
    return;
  }

  let html = "";
  ports.forEach(p => {
    let statusClass = "color: var(--emerald-recv);";
    if (p.status.includes("In Use")) statusClass = "color: var(--amber-avoid);";
    if (p.status.includes("Check Driver")) statusClass = "color: var(--text-dim);";

    let btnHtml = "";
    if (p.status === "Available") {
      btnHtml = `
        <button class="btn btn-outline" style="padding: 4px 10px; font-size: 11px;" onclick="applyAutoDetectedPort('${p.port}', ${p.baud_rate}, '${p.model}', 'scanner_a')">Use Scanner A</button>
        <button class="btn btn-outline" style="padding: 4px 10px; font-size: 11px;" onclick="applyAutoDetectedPort('${p.port}', ${p.baud_rate}, '${p.model}', 'scanner_b')">Use Scanner B</button>
      `;
    } else {
      btnHtml = `<span style="font-size: 11px; color: var(--text-dim);">Disabled</span>`;
    }

    html += `
      <tr>
        <td><strong style="color: #ffffff;">${p.port}</strong></td>
        <td><span style="${statusClass} font-weight: 600;">${p.status}</span></td>
        <td><span style="color: var(--cyan-accent); font-weight: 600;">${p.model || 'Unknown Radio'}</span></td>
        <td><code>${p.baud_rate}</code></td>
        <td style="display: flex; gap: 6px;">${btnHtml}</td>
      </tr>
    `;
  });

  body.innerHTML = html;
}

function applyAutoDetectedPort(port, baudRate, detectedModel, targetScannerId) {
  if (!currentAppState || !currentAppState.config) return;

  const cfg = JSON.parse(JSON.stringify(currentAppState.config));
  const target = targetScannerId === 'scanner_a' ? cfg.scanner_a : cfg.scanner_b;

  target.port = port;
  if (baudRate > 0) target.baud_rate = baudRate;
  if (detectedModel && modelsCatalog.some(m => m.model === detectedModel)) {
    target.model = detectedModel;
  }
  target.connection_mode = "SERIAL";

  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cfg)
  }).then(() => {
    closeAutoDetectModal();
    alert(`Assigned ${port} (${detectedModel || 'Serial'}) to ${targetScannerId.toUpperCase()}!`);
  });
}

function runLANDiscovery() {
  const body = document.getElementById("autoDetectTableBody");
  if (body) {
    body.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-dim); padding: 20px;">Broadcasting UDP discovery query (SUS,UNIDEN,SCANNER) on port 50536...</td></tr>`;
  }

  fetch('/api/scanners/discover_lan')
    .then(res => res.json())
    .then(data => {
      if (data.status === "success" && data.scanners.length > 0) {
        renderLANDiscoveryGrid(data.scanners);
      } else {
        if (body) body.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-dim); padding: 20px;">No network IP scanners replied on UDP port 50536.</td></tr>`;
      }
    });
}

function renderLANDiscoveryGrid(scanners) {
  const body = document.getElementById("autoDetectTableBody");
  if (!body) return;

  let html = "";
  scanners.forEach(s => {
    html += `
      <tr>
        <td><strong style="color: var(--cyan-accent);">${s.ip_address}</strong></td>
        <td><span style="color: var(--emerald-recv); font-weight: 600;">UDP Discovered</span></td>
        <td><span style="color: #ffffff; font-weight: 600;">${s.model} (${s.name})</span></td>
        <td><code>UDP 50536</code></td>
        <td style="display: flex; gap: 6px;">
          <button class="btn btn-outline" style="padding: 4px 10px; font-size: 11px;" onclick="applyLANScanner('${s.ip_address}', '${s.model}', 'scanner_a')">Use Scanner A</button>
          <button class="btn btn-outline" style="padding: 4px 10px; font-size: 11px;" onclick="applyLANScanner('${s.ip_address}', '${s.model}', 'scanner_b')">Use Scanner B</button>
        </td>
      </tr>
    `;
  });
  body.innerHTML = html;
}

function applyLANScanner(ipHost, detectedModel, targetScannerId) {
  if (!currentAppState || !currentAppState.config) return;

  const cfg = JSON.parse(JSON.stringify(currentAppState.config));
  const target = targetScannerId === 'scanner_a' ? cfg.scanner_a : cfg.scanner_b;

  target.ip_host = ipHost;
  target.connection_mode = "IP";
  if (detectedModel && modelsCatalog.some(m => m.model === detectedModel)) {
    target.model = detectedModel;
  }

  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cfg)
  }).then(() => {
    closeAutoDetectModal();
    alert(`Assigned IP ${ipHost} to ${targetScannerId.toUpperCase()}!`);
  });
}

function sendSkip(scannerId) {
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ type: "SKIP", scanner_id: scannerId }));
  } else {
    fetch(`/api/skip/${scannerId}`, { method: 'POST' });
  }
}

function toggleMasterSync(enabled) {
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify({ type: "TOGGLE_SYNC", enabled: enabled }));
  } else {
    if (currentAppState && currentAppState.config) {
      currentAppState.config.sync.enable_sync = enabled;
      fetch('/api/config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(currentAppState.config)
      });
    }
  }
}

function updateEngineRules() {
  const exclMode = document.getElementById("exclusionModeSelect").value;
  const prioMode = document.getElementById("priorityModeSelect").value;
  const primaryId = document.getElementById("primaryScannerSelect").value;

  if (currentAppState && currentAppState.config) {
    const updated = JSON.parse(JSON.stringify(currentAppState.config));
    updated.sync.exclusion_mode = exclMode;
    updated.sync.priority_mode = prioMode;
    updated.sync.primary_scanner_id = primaryId;

    fetch('/api/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(updated)
    });
  }
}

function openSettingsModal() {
  const modal = document.getElementById("settingsModal");
  if (modal) {
    modal.classList.add("active");
    loadAvailableComPorts(() => {
      if (currentAppState && currentAppState.config) {
        const cfg = currentAppState.config;
        document.getElementById("cfg_model_a").value = cfg.scanner_a.model;
        document.getElementById("cfg_mode_a").value = cfg.scanner_a.connection_mode;
        
        document.getElementById("cfg_model_b").value = cfg.scanner_b.model;
        document.getElementById("cfg_mode_b").value = cfg.scanner_b.connection_mode;

        // Populate Ports
        populateComPortDropdowns();

        const portA = cfg.scanner_a.port;
        const portB = cfg.scanner_b.port;

        const selA = document.getElementById("cfg_port_a");
        const customA = document.getElementById("cfg_custom_port_a");
        if ([...selA.options].some(o => o.value === portA)) {
          selA.value = portA;
          customA.style.display = "none";
        } else {
          selA.value = "CUSTOM";
          customA.value = portA;
          customA.style.display = "block";
        }

        const selB = document.getElementById("cfg_port_b");
        const customB = document.getElementById("cfg_custom_port_b");
        if ([...selB.options].some(o => o.value === portB)) {
          selB.value = portB;
          customB.style.display = "none";
        } else {
          selB.value = "CUSTOM";
          customB.value = portB;
          customB.style.display = "block";
        }

        toggleConnectionModeUI('a');
        toggleConnectionModeUI('b');

        updateBaudOptions('a');
        updateBaudOptions('b');

        if (cfg.feeder) {
          document.getElementById("cfg_feeder_mode").value = cfg.feeder.feeder_mode;
          document.getElementById("cfg_feeder_format").value = cfg.feeder.audio_format;
          document.getElementById("cfg_feeder_inbox").value = cfg.feeder.inbox_directory;
        }

        loadAudioDevices();
      }
    });
  }
}

function closeSettingsModal() {
  const modal = document.getElementById("settingsModal");
  if (modal) modal.classList.remove("active");
}

function getSelectedPort(suffix) {
  const mode = document.getElementById(`cfg_mode_${suffix}`).value;
  const sel = document.getElementById(`cfg_port_${suffix}`);
  const custom = document.getElementById(`cfg_custom_port_${suffix}`);

  if (mode === "IP") {
    return custom.value.trim() || "127.0.0.1";
  }

  if (sel.value === "CUSTOM") {
    return custom.value.trim() || "COM1";
  }

  return sel.value;
}

function saveConfiguration(event) {
  event.preventDefault();

  if (!currentAppState || !currentAppState.config) return;

  const cfg = JSON.parse(JSON.stringify(currentAppState.config));

  cfg.scanner_a.model = document.getElementById("cfg_model_a").value;
  cfg.scanner_a.connection_mode = document.getElementById("cfg_mode_a").value;
  cfg.scanner_a.port = getSelectedPort('a');
  cfg.scanner_a.baud_rate = parseInt(document.getElementById("cfg_baud_a").value);

  cfg.scanner_b.model = document.getElementById("cfg_model_b").value;
  cfg.scanner_b.connection_mode = document.getElementById("cfg_mode_b").value;
  cfg.scanner_b.port = getSelectedPort('b');
  cfg.scanner_b.baud_rate = parseInt(document.getElementById("cfg_baud_b").value);

  // Scanner A Audio
  const audDevA = document.getElementById("cfg_audio_dev_a").value;
  cfg.scanner_a.audio_enabled = (audDevA !== "DISABLED");
  cfg.scanner_a.audio_device_index = (audDevA === "DISABLED" || audDevA === "") ? null : parseInt(audDevA);
  cfg.scanner_a.audio_channel = document.getElementById("cfg_audio_ch_a").value;

  // Scanner B Audio
  const audDevB = document.getElementById("cfg_audio_dev_b").value;
  cfg.scanner_b.audio_enabled = (audDevB !== "DISABLED");
  cfg.scanner_b.audio_device_index = (audDevB === "DISABLED" || audDevB === "") ? null : parseInt(audDevB);
  cfg.scanner_b.audio_channel = document.getElementById("cfg_audio_ch_b").value;

  if (cfg.feeder) {
    cfg.feeder.feeder_mode = document.getElementById("cfg_feeder_mode").value;
    cfg.feeder.audio_format = document.getElementById("cfg_feeder_format").value;
    cfg.feeder.inbox_directory = document.getElementById("cfg_feeder_inbox").value;
  }

  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cfg)
  }).then(() => {
    closeSettingsModal();
  });
}

let availableAudioDevices = [];

function loadAudioDevices(callback) {
  fetch('/api/audio/devices')
    .then(res => res.json())
    .then(data => {
      if (data.status === "success") {
        availableAudioDevices = data.devices || [];
        populateAudioDeviceDropdowns();
        if (callback) callback();
      }
    })
    .catch(err => console.error("Failed to load audio devices:", err));
}

function populateAudioDeviceDropdowns() {
  const selA = document.getElementById("cfg_audio_dev_a");
  const selB = document.getElementById("cfg_audio_dev_b");
  if (!selA || !selB) return;

  function buildAudioOptions(currentVal, isEnabled) {
    let html = `<option value="DISABLED" ${!isEnabled ? "selected" : ""}>Disabled / No Audio Capture</option>`;
    html += `<option value="" ${isEnabled && (currentVal === null || currentVal === undefined) ? "selected" : ""}>Default System Audio Input</option>`;
    availableAudioDevices.forEach(d => {
      const selected = (isEnabled && currentVal !== null && currentVal !== undefined && parseInt(currentVal) === d.index) ? "selected" : "";
      const isDef = d.is_default ? " (System Default)" : "";
      html += `<option value="${d.index}" ${selected}>${d.name} (${d.channels} ch)${isDef}</option>`;
    });
    return html;
  }

  const cfgA = currentAppState && currentAppState.config ? currentAppState.config.scanner_a : {};
  const cfgB = currentAppState && currentAppState.config ? currentAppState.config.scanner_b : {};

  selA.innerHTML = buildAudioOptions(cfgA.audio_device_index, cfgA.audio_enabled !== false);
  selB.innerHTML = buildAudioOptions(cfgB.audio_device_index, cfgB.audio_enabled !== false);

  const chA = document.getElementById("cfg_audio_ch_a");
  const chB = document.getElementById("cfg_audio_ch_b");
  if (chA && cfgA.audio_channel) chA.value = cfgA.audio_channel;
  if (chB && cfgB.audio_channel) chB.value = cfgB.audio_channel;
}

function testScanScribeFeeder() {
  fetch('/api/feeder/dispatch_test', { method: 'POST' })
    .then(res => res.json())
    .then(data => {
      if (data.status === "success" && data.dispatch_result.status === "success") {
        alert(`Dispatched Test Audio Feed to ScanScribe Inbox!\nAudio: ${data.dispatch_result.audio_file}`);
      } else {
        alert(`Feeder Dispatch Result: ${JSON.stringify(data.dispatch_result)}`);
      }
    });
}

window.addEventListener("DOMContentLoaded", () => {
  connectWebSocket();
  loadModelsCatalog();
  loadAvailableComPorts();
  loadAudioDevices();
});
