let socket = null;
let currentAppState = null;
let pollingInterval = null;
let modelsCatalog = [];
let availableComPorts = [];
let availableAudioDevices = [];
let availableOutputDevices = [];
let clearedEventTimestamp = 0;

const PROSCAN_SPECIFIERS = [
  { code: "%DT", desc: "Date & Time (YYYY-MM-DD HH-MM-SS)" },
  { code: "%D1", desc: "Date (YYYY-MM-DD)" },
  { code: "%T1", desc: "Time 24h (HH-MM-SS)" },
  { code: "%Y", desc: "Year (4-digit)" },
  { code: "%Y1", desc: "Year (4-digit)" },
  { code: "%Y2", desc: "Year (2-digit)" },
  { code: "%M", desc: "Month (01-12)" },
  { code: "%M1", desc: "Month (01-12)" },
  { code: "%M2", desc: "Month Abbrev (Jan-Dec)" },
  { code: "%M3", desc: "Month Name (January..)" },
  { code: "%D", desc: "Day of Month (01-31)" },
  { code: "%D2", desc: "Weekday Abbrev (Mon-Sun)" },
  { code: "%D3", desc: "Weekday Name (Monday..)" },
  { code: "%H", desc: "Hour 24h (00-23)" },
  { code: "%H1", desc: "Hour 24h (00-23)" },
  { code: "%H2", desc: "Hour 12h (01-12)" },
  { code: "%I", desc: "Hour 12h (01-12)" },
  { code: "%N", desc: "Minute (00-59)" },
  { code: "%M4", desc: "Minute (00-59)" },
  { code: "%S1", desc: "Second (00-59)" },
  { code: "%P", desc: "AM / PM" },
  { code: "%AMPM", desc: "AM / PM" },
  { code: "%S", desc: "System Name" },
  { code: "%G", desc: "Department / Group Name" },
  { code: "%C", desc: "Channel Name" },
  { code: "%F", desc: "Frequency (0155.7975)" },
  { code: "%TG", desc: "TalkGroup ID (TGID)" },
  { code: "%TF", desc: "TGID or Frequency" },
  { code: "%FT", desc: "Frequency or TGID" },
  { code: "%U", desc: "Unit ID (UID)" },
  { code: "%U1", desc: "Unit ID Name" },
  { code: "%T", desc: "Tone (CTCSS/DCS/NAC)" },
  { code: "%MD", desc: "Modulation (NFM/P25/AM)" },
  { code: "%A", desc: "Site Name" },
  { code: "%N1", desc: "System Number" },
  { code: "%N2", desc: "Department Number" },
  { code: "%N3", desc: "Channel Number" }
];

function renderSpecifiersReference() {
  const grid = document.getElementById("specifiersGrid");
  if (!grid) return;
  grid.innerHTML = PROSCAN_SPECIFIERS.map(s => `
    <div class="spec-item">
      <span class="spec-code">${s.code}</span>
      <span class="spec-desc">${s.desc}</span>
    </div>
  `).join("");
}

function formatPreviewTemplate(tpl, scannerLabel) {
  if (!tpl) return "";
  return tpl
    .replace(/%DT/g, "2026-11-08 16-30-00")
    .replace(/%D1/g, "2026-11-08")
    .replace(/%T1/g, "16-30-00")
    .replace(/%Y1/g, "2026")
    .replace(/%Y2/g, "26")
    .replace(/%Y/g, "2026")
    .replace(/%M4/g, "30")
    .replace(/%M3/g, "November")
    .replace(/%M2/g, "Nov")
    .replace(/%M1/g, "11")
    .replace(/%M/g, "11")
    .replace(/%D3/g, "Sunday")
    .replace(/%D2/g, "Sun")
    .replace(/%D/g, "08")
    .replace(/%H2/g, "04")
    .replace(/%H1/g, "16")
    .replace(/%H/g, "16")
    .replace(/%I/g, "04")
    .replace(/%N1/g, "1")
    .replace(/%N2/g, "10")
    .replace(/%N3/g, "104")
    .replace(/%N/g, "30")
    .replace(/%S1/g, "00")
    .replace(/%AMPM/g, "PM")
    .replace(/%P/g, "PM")
    .replace(/%S/g, scannerLabel === "b" ? "County Simulcast" : "Metro P25")
    .replace(/%G/g, scannerLabel === "b" ? "Sheriff Primary" : "Police Dispatch")
    .replace(/%C/g, scannerLabel === "b" ? "SO Dispatch 1" : "PD North Disp")
    .replace(/%TF/g, "1024")
    .replace(/%FT/g, "0851.2500")
    .replace(/%TG/g, "1024")
    .replace(/%F/g, "0851.2500")
    .replace(/%U1/g, "Unit 402")
    .replace(/%U/g, "44021")
    .replace(/%MD/g, "P25")
    .replace(/%T/g, "NAC 293")
    .replace(/%A/g, "North Simulcast");
}

function applyTemplatePreset(suffix, fnPattern, tit2Pattern) {
  const fnInput = document.getElementById(`cfg_filename_template_${suffix}`);
  const tit2Input = document.getElementById(`cfg_tit2_template_${suffix}`);
  if (fnInput) fnInput.value = fnPattern;
  if (tit2Input) tit2Input.value = tit2Pattern;
  updateTemplatePreviews();
}

function updateTemplatePreviews() {
  const fmtSel = document.getElementById("cfg_feeder_format");
  const ext = (fmtSel && fmtSel.value === "MP3") ? ".mp3" : ".wav";

  ["a", "b"].forEach(suffix => {
    const fnInput = document.getElementById(`cfg_filename_template_${suffix}`);
    const tit2Input = document.getElementById(`cfg_tit2_template_${suffix}`);
    const fnPrev = document.getElementById(`preview_fn_${suffix}`);
    const tit2Prev = document.getElementById(`preview_tit2_${suffix}`);

    if (fnInput && fnPrev) {
      const val = fnInput.value.trim() || "%DT - %S - %C (%TG)";
      fnPrev.innerText = `Preview: ${formatPreviewTemplate(val, suffix)}${ext}`;
    }
    if (tit2Input && tit2Prev) {
      const val = tit2Input.value.trim() || "%C (%TG)";
      tit2Prev.innerText = `TIT2: ${formatPreviewTemplate(val, suffix)}`;
    }
  });
}

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
    let html = `<option value="NONE" ${currentVal === "NONE" ? "selected" : ""}>NONE (Disabled)</option>`;
    if (availableComPorts.length > 0) {
      availableComPorts.forEach(p => {
        const selected = (p.device === currentVal) ? "selected" : "";
        html += `<option value="${p.device}" ${selected}>${p.label}</option>`;
      });
    }
    if (currentVal && !availableComPorts.some(p => p.device === currentVal) && currentVal !== "NONE") {
      html = `<option value="${currentVal}" selected>${currentVal} (Configured)</option>` + html;
    }
    html += `<option value="CUSTOM">Manual / Custom Port...</option>`;
    return html;
  }

  const curA = currentAppState && currentAppState.config ? currentAppState.config.scanner_a.port : "COM5";
  const curB = currentAppState && currentAppState.config ? currentAppState.config.scanner_b.port : "NONE";

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
  const levels = state.audio_levels || { scanner_a: 0, scanner_b: 0 };
  const activeExclusions = state.active_exclusions || {};

  // Update Model Selects / Badges
  const cardSelA = document.getElementById("cardModelSelectA");
  const cardSelB = document.getElementById("cardModelSelectB");
  if (cardSelA && scA.model && document.activeElement !== cardSelA) {
    cardSelA.value = scA.model;
  }
  if (cardSelB && scB.model && document.activeElement !== cardSelB) {
    cardSelB.value = scB.model;
  }

  // 1. Master Sync & Record Toggles
  const syncToggle = document.getElementById("masterSyncToggle");
  if (syncToggle && cfg.sync) {
    syncToggle.checked = cfg.sync.enable_sync;
  }

  const recordToggle = document.getElementById("masterRecordToggle");
  const recordingEnabled = cfg.feeder ? cfg.feeder.enable_feeder !== false : true;
  if (recordToggle) {
    recordToggle.checked = recordingEnabled;
  }

  // Global Recording Status Badge + Document Title Indicator
  const recA = recordingEnabled && cfg.scanner_a && cfg.scanner_a.enabled && cfg.scanner_a.audio_enabled && scA.receiving && !scA.avoided;
  const recB = recordingEnabled && cfg.scanner_b && cfg.scanner_b.enabled && cfg.scanner_b.audio_enabled && scB.receiving && !scB.avoided;
  const globalRecBadge = document.getElementById("globalRecBadge");
  if (globalRecBadge) {
    if (!recordingEnabled) {
      globalRecBadge.className = "rec-badge rec-off";
      globalRecBadge.innerText = "● REC DISABLED";
      document.title = "Uniden Scanner Mutual Exclusion Controller";
    } else if (recA || recB) {
      globalRecBadge.className = "rec-badge rec-active";
      globalRecBadge.innerText = "🔴 REC ACTIVE";
      document.title = "🔴 [REC] Uniden Scanner Mutual Exclusion Controller";
    } else {
      globalRecBadge.className = "rec-badge rec-on";
      globalRecBadge.innerText = "● REC STANDBY";
      document.title = "Uniden Scanner Mutual Exclusion Controller";
    }
  }

  // 2. Rules Controls
  if (cfg.sync) {
    const exclEl = document.getElementById("exclusionModeSelect");
    const prioEl = document.getElementById("priorityModeSelect");
    const primEl = document.getElementById("primaryScannerSelect");
    const hangEl = document.getElementById("hangTimeInput");
    const autoRelEl = document.getElementById("autoReleaseInput");

    if (exclEl && document.activeElement !== exclEl) exclEl.value = cfg.sync.exclusion_mode;
    if (prioEl && document.activeElement !== prioEl) prioEl.value = cfg.sync.priority_mode;
    if (primEl && document.activeElement !== primEl) primEl.value = cfg.sync.primary_scanner_id;
    if (hangEl && document.activeElement !== hangEl) hangEl.value = cfg.sync.hang_time_seconds ?? 2.0;
    if (autoRelEl && document.activeElement !== autoRelEl) autoRelEl.value = cfg.sync.auto_release_seconds ?? 30.0;
  }

  // 3. Active Holds Bar
  renderActiveHolds(activeExclusions);

  // 4. Loopback Monitor Panel
  if (cfg.audio_loopback) {
    const lbToggle = document.getElementById("loopbackEnabledToggle");
    const lbPan = document.getElementById("loopbackPanSelect");
    const lbVol = document.getElementById("loopbackVolumeSlider");
    const lbVolLbl = document.getElementById("loopbackVolumeLabel");
    const lbOut = document.getElementById("loopbackOutputSelect");

    if (lbToggle && document.activeElement !== lbToggle) {
      lbToggle.checked = cfg.audio_loopback.enabled !== false;
    }
    if (lbPan && document.activeElement !== lbPan) {
      lbPan.value = cfg.audio_loopback.pan_mode || "STEREO_SPLIT";
    }
    const pct = Math.round((cfg.audio_loopback.volume ?? 0.8) * 100);
    if (lbVol && document.activeElement !== lbVol) {
      lbVol.value = pct;
      if (lbVolLbl) lbVolLbl.innerText = `${pct}%`;
    }
    if (lbOut && document.activeElement !== lbOut) {
      const targetVal = (cfg.audio_loopback.output_device_index === null || cfg.audio_loopback.output_device_index === undefined)
        ? ""
        : String(cfg.audio_loopback.output_device_index);
      if ([...lbOut.options].some(o => o.value === targetVal)) {
        lbOut.value = targetVal;
      }
    }
  }

  // 5. Render Scanner A & B Cards (with VU meters and REC badges)
  renderScannerCard("A", scA, cfg.scanner_a || {}, recordingEnabled, levels.scanner_a || 0);
  renderScannerCard("B", scB, cfg.scanner_b || {}, recordingEnabled, levels.scanner_b || 0);

  // 6. Render Event Log Table
  renderLogTable(events);
}

function renderActiveHolds(activeExclusions) {
  const bar = document.getElementById("activeExclusionsBar");
  if (!bar) return;
  const keys = Object.keys(activeExclusions || {});
  if (keys.length === 0) {
    bar.innerHTML = `
      <span style="color: var(--text-muted); font-weight: 600;">Active Holds:</span>
      <span style="color: var(--text-dim);">None — Both scanners free</span>
    `;
    return;
  }

  const pills = keys.map(k => {
    const entry = activeExclusions[k];
    const owner = (entry.owner || "").toUpperCase();
    return `<span class="hold-pill">${k} (${owner})</span>`;
  }).join("");

  bar.innerHTML = `<span style="color: var(--text-muted); font-weight: 600;">Active Holds (${keys.length}):</span> ${pills}`;
}

function renderScannerCard(suffix, data, scannerCfg, masterRecordEnabled, audioLevelPct) {
  const card = document.getElementById(`card${suffix}`);
  const badge = document.getElementById(`badge${suffix}`);
  const statusText = document.getElementById(`statusText${suffix}`);
  const recBadge = document.getElementById(`recBadge${suffix}`);

  const sysEl = document.getElementById(`sys${suffix}`);
  const deptEl = document.getElementById(`dept${suffix}`);
  const chanEl = document.getElementById(`chan${suffix}`);
  const tgidEl = document.getElementById(`tgid${suffix}`);
  const freqEl = document.getElementById(`freq${suffix}`);
  const modEl = document.getElementById(`mod${suffix}`);
  const uidEl = document.getElementById(`uid${suffix}`);
  const rssiContainer = document.getElementById(`rssi${suffix}`);

  const vuBar = document.getElementById(`vuBar${suffix}`);
  const vuPct = document.getElementById(`vuPct${suffix}`);
  const cardEnable = document.getElementById(`cardEnable${suffix}`);
  const cardAudioEnable = document.getElementById(`cardAudioEnable${suffix}`);

  if (!card) return;

  sysEl.innerText = data.system_name || "Scanning...";
  deptEl.innerText = data.dept_name || "";
  chanEl.innerText = data.channel_name || "Scanning...";
  tgidEl.innerText = data.tgid || "--";
  freqEl.innerText = data.frequency ? `${data.frequency.toFixed(4)} MHz` : "-- MHz";
  modEl.innerText = data.modulation || "NFM";
  if (uidEl) uidEl.innerText = data.unit_id || "--";

  if (cardEnable && document.activeElement !== cardEnable) {
    cardEnable.checked = scannerCfg.enabled !== false;
  }
  if (cardAudioEnable && document.activeElement !== cardAudioEnable) {
    cardAudioEnable.checked = scannerCfg.audio_enabled !== false;
  }

  // Per-card REC Badge (gui.py parity: REC OFF / REC ON / 🔴 REC)
  if (recBadge) {
    const canRecord = masterRecordEnabled && scannerCfg.enabled !== false && scannerCfg.audio_enabled !== false;
    if (!canRecord) {
      recBadge.className = "rec-badge rec-off";
      recBadge.innerText = "REC OFF";
    } else if (data.receiving && !data.avoided) {
      recBadge.className = "rec-badge rec-active";
      recBadge.innerText = "🔴 REC";
    } else {
      recBadge.className = "rec-badge rec-on";
      recBadge.innerText = "REC ON";
    }
  }

  // Live Audio VU Meter (gui.py:924-953 color thresholds: <60% green, 60-84% amber, >=85% red)
  const clampedLevel = Math.max(0, Math.min(100, Number(audioLevelPct) || 0));
  if (vuBar) {
    vuBar.style.width = `${clampedLevel}%`;
    if (clampedLevel >= 85) {
      vuBar.className = "vu-bar-fill vu-peak";
    } else if (clampedLevel >= 60) {
      vuBar.className = "vu-bar-fill vu-warn";
    } else {
      vuBar.className = "vu-bar-fill";
    }
  }
  if (vuPct) {
    vuPct.innerText = `${clampedLevel}%`;
  }

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

function clearLogView() {
  if (currentAppState && currentAppState.event_log && currentAppState.event_log.length > 0) {
    const maxTs = Math.max(...currentAppState.event_log.map(e => Number(e.timestamp) || 0));
    clearedEventTimestamp = maxTs;
    renderLogTable(currentAppState.event_log);
  }
}

function renderLogTable(events) {
  const body = document.getElementById("logTableBody");
  const countEl = document.getElementById("logCount");

  if (!body) return;

  const filtered = (events || []).filter(e => (Number(e.timestamp) || 0) > clearedEventTimestamp);

  if (filtered.length === 0) {
    body.innerHTML = `<tr><td colspan="6" style="text-align: center; color: var(--text-dim); padding: 20px;">No mutual exclusion events recorded yet.</td></tr>`;
    if (countEl) countEl.innerText = "0 events recorded";
    return;
  }

  if (countEl) countEl.innerText = `${filtered.length} events recorded`;

  let html = "";
  filtered.forEach(e => {
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
  target.enabled = true;
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
  target.enabled = true;
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
  } else if (currentAppState && currentAppState.config) {
    currentAppState.config.sync.enable_sync = enabled;
    fetch('/api/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(currentAppState.config)
    });
  }
}

function toggleMasterRecord(enabled) {
  if (!currentAppState || !currentAppState.config) return;
  const updated = JSON.parse(JSON.stringify(currentAppState.config));
  updated.feeder.enable_feeder = enabled;
  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updated)
  });
}

function toggleCardScannerEnabled(scannerId, enabled) {
  if (!currentAppState || !currentAppState.config) return;
  const updated = JSON.parse(JSON.stringify(currentAppState.config));
  if (scannerId === 'scanner_a') {
    updated.scanner_a.enabled = enabled;
  } else {
    updated.scanner_b.enabled = enabled;
  }
  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updated)
  });
}

function toggleCardAudioEnabled(scannerId, enabled) {
  if (!currentAppState || !currentAppState.config) return;
  const updated = JSON.parse(JSON.stringify(currentAppState.config));
  if (scannerId === 'scanner_a') {
    updated.scanner_a.audio_enabled = enabled;
  } else {
    updated.scanner_b.audio_enabled = enabled;
  }
  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updated)
  });
}

function previewLoopbackVolume(val) {
  const lbl = document.getElementById("loopbackVolumeLabel");
  if (lbl) lbl.innerText = `${val}%`;
}

function updateLoopbackControls() {
  if (!currentAppState || !currentAppState.config) return;
  const updated = JSON.parse(JSON.stringify(currentAppState.config));
  if (!updated.audio_loopback) {
    updated.audio_loopback = { enabled: true, output_device_index: null, volume: 0.8, pan_mode: "STEREO_SPLIT" };
  }

  const lbToggle = document.getElementById("loopbackEnabledToggle");
  const lbOut = document.getElementById("loopbackOutputSelect");
  const lbPan = document.getElementById("loopbackPanSelect");
  const lbVol = document.getElementById("loopbackVolumeSlider");

  if (lbToggle) updated.audio_loopback.enabled = lbToggle.checked;
  if (lbOut) {
    updated.audio_loopback.output_device_index = (lbOut.value === "") ? null : parseInt(lbOut.value, 10);
  }
  if (lbPan) updated.audio_loopback.pan_mode = lbPan.value;
  if (lbVol) updated.audio_loopback.volume = Math.round(parseFloat(lbVol.value)) / 100.0;

  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updated)
  });
}

function updateEngineRules() {
  const exclMode = document.getElementById("exclusionModeSelect").value;
  const prioMode = document.getElementById("priorityModeSelect").value;
  const primaryId = document.getElementById("primaryScannerSelect").value;
  const hangEl = document.getElementById("hangTimeInput");
  const autoRelEl = document.getElementById("autoReleaseInput");

  if (currentAppState && currentAppState.config) {
    const updated = JSON.parse(JSON.stringify(currentAppState.config));
    updated.sync.exclusion_mode = exclMode;
    updated.sync.priority_mode = prioMode;
    updated.sync.primary_scanner_id = primaryId;
    if (hangEl) {
      const h = parseFloat(hangEl.value);
      if (!isNaN(h) && h >= 0) updated.sync.hang_time_seconds = h;
    }
    if (autoRelEl) {
      const a = parseFloat(autoRelEl.value);
      if (!isNaN(a) && a >= 1) updated.sync.auto_release_seconds = a;
    }

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
        if (document.getElementById("cfg_lockout_a")) {
          document.getElementById("cfg_lockout_a").value = cfg.scanner_a.lockout_method || "KEYPRESS";
        }

        document.getElementById("cfg_model_b").value = cfg.scanner_b.model;
        document.getElementById("cfg_mode_b").value = cfg.scanner_b.connection_mode;
        if (document.getElementById("cfg_lockout_b")) {
          document.getElementById("cfg_lockout_b").value = cfg.scanner_b.lockout_method || "KEYPRESS";
        }

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

        if (document.getElementById("cfg_filename_template_a")) {
          document.getElementById("cfg_filename_template_a").value = cfg.scanner_a.filename_template || "%DT - %S - %C (%TG)";
        }
        if (document.getElementById("cfg_tit2_template_a")) {
          document.getElementById("cfg_tit2_template_a").value = cfg.scanner_a.tit2_template || "%C (%TG)";
        }

        if (document.getElementById("cfg_filename_template_b")) {
          document.getElementById("cfg_filename_template_b").value = cfg.scanner_b.filename_template || "%DT - %S - %C (%TG)";
        }
        if (document.getElementById("cfg_tit2_template_b")) {
          document.getElementById("cfg_tit2_template_b").value = cfg.scanner_b.tit2_template || "%C (%TG)";
        }

        if (cfg.feeder) {
          document.getElementById("cfg_feeder_mode").value = cfg.feeder.feeder_mode;
          document.getElementById("cfg_feeder_format").value = cfg.feeder.audio_format;
          if (document.getElementById("cfg_feeder_sample_rate")) {
            document.getElementById("cfg_feeder_sample_rate").value = String(cfg.feeder.sample_rate || 16000);
          }
          document.getElementById("cfg_feeder_inbox").value = cfg.feeder.inbox_directory;
          if (document.getElementById("cfg_feeder_webhook")) {
            document.getElementById("cfg_feeder_webhook").value = cfg.feeder.webhook_url || "http://127.0.0.1:5000/api/ingest";
          }
          if (document.getElementById("cfg_feeder_proscan_tags")) {
            document.getElementById("cfg_feeder_proscan_tags").checked = cfg.feeder.enable_proscan_tags !== false;
          }
          if (document.getElementById("cfg_feeder_sidecar_json")) {
            document.getElementById("cfg_feeder_sidecar_json").checked = cfg.feeder.enable_sidecar_json !== false;
          }
        }

        updateTemplatePreviews();
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
  cfg.scanner_a.baud_rate = parseInt(document.getElementById("cfg_baud_a").value, 10);
  if (document.getElementById("cfg_lockout_a")) {
    cfg.scanner_a.lockout_method = document.getElementById("cfg_lockout_a").value;
  }

  cfg.scanner_b.model = document.getElementById("cfg_model_b").value;
  cfg.scanner_b.connection_mode = document.getElementById("cfg_mode_b").value;
  cfg.scanner_b.port = getSelectedPort('b');
  cfg.scanner_b.baud_rate = parseInt(document.getElementById("cfg_baud_b").value, 10);
  if (document.getElementById("cfg_lockout_b")) {
    cfg.scanner_b.lockout_method = document.getElementById("cfg_lockout_b").value;
  }

  // Scanner A Audio
  const audDevA = document.getElementById("cfg_audio_dev_a").value;
  cfg.scanner_a.audio_enabled = (audDevA !== "DISABLED");
  cfg.scanner_a.audio_device_index = (audDevA === "DISABLED" || audDevA === "") ? null : parseInt(audDevA, 10);
  cfg.scanner_a.audio_channel = document.getElementById("cfg_audio_ch_a").value;

  // Scanner B Audio
  const audDevB = document.getElementById("cfg_audio_dev_b").value;
  cfg.scanner_b.audio_enabled = (audDevB !== "DISABLED");
  cfg.scanner_b.audio_device_index = (audDevB === "DISABLED" || audDevB === "") ? null : parseInt(audDevB, 10);
  cfg.scanner_b.audio_channel = document.getElementById("cfg_audio_ch_b").value;

  // Scanner A Templates
  if (document.getElementById("cfg_filename_template_a")) {
    cfg.scanner_a.filename_template = document.getElementById("cfg_filename_template_a").value.trim() || "%DT - %S - %C (%TG)";
  }
  if (document.getElementById("cfg_tit2_template_a")) {
    cfg.scanner_a.tit2_template = document.getElementById("cfg_tit2_template_a").value.trim() || "%C (%TG)";
  }

  // Scanner B Templates
  if (document.getElementById("cfg_filename_template_b")) {
    cfg.scanner_b.filename_template = document.getElementById("cfg_filename_template_b").value.trim() || "%DT - %S - %C (%TG)";
  }
  if (document.getElementById("cfg_tit2_template_b")) {
    cfg.scanner_b.tit2_template = document.getElementById("cfg_tit2_template_b").value.trim() || "%C (%TG)";
  }

  if (cfg.feeder) {
    cfg.feeder.feeder_mode = document.getElementById("cfg_feeder_mode").value;
    cfg.feeder.audio_format = document.getElementById("cfg_feeder_format").value;
    if (document.getElementById("cfg_feeder_sample_rate")) {
      cfg.feeder.sample_rate = parseInt(document.getElementById("cfg_feeder_sample_rate").value, 10);
    }
    cfg.feeder.inbox_directory = document.getElementById("cfg_feeder_inbox").value;
    if (document.getElementById("cfg_feeder_webhook")) {
      cfg.feeder.webhook_url = document.getElementById("cfg_feeder_webhook").value.trim() || "http://127.0.0.1:5000/api/ingest";
    }
    if (document.getElementById("cfg_feeder_proscan_tags")) {
      cfg.feeder.enable_proscan_tags = document.getElementById("cfg_feeder_proscan_tags").checked;
    }
    if (document.getElementById("cfg_feeder_sidecar_json")) {
      cfg.feeder.enable_sidecar_json = document.getElementById("cfg_feeder_sidecar_json").checked;
    }
  }

  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cfg)
  }).then(() => {
    closeSettingsModal();
  });
}

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

function loadAudioOutputDevices(notifyUser) {
  fetch('/api/audio/output_devices')
    .then(res => res.json())
    .then(data => {
      if (data.status === "success") {
        availableOutputDevices = data.devices || [];
        populateAudioOutputDropdown();
        if (notifyUser) {
          alert(`Refreshed output speakers!\nFound ${availableOutputDevices.length} output device(s).`);
        }
      }
    })
    .catch(err => console.error("Failed to load audio output devices:", err));
}

function populateAudioOutputDropdown() {
  const sel = document.getElementById("loopbackOutputSelect");
  if (!sel) return;

  const curIdx = currentAppState && currentAppState.config && currentAppState.config.audio_loopback
    ? currentAppState.config.audio_loopback.output_device_index
    : null;

  let html = `<option value="" ${(curIdx === null || curIdx === undefined) ? "selected" : ""}>Default System Output Speaker</option>`;
  availableOutputDevices.forEach(d => {
    const selected = (curIdx !== null && curIdx !== undefined && parseInt(curIdx, 10) === d.index) ? "selected" : "";
    const isDef = d.is_default ? " (System Default)" : "";
    html += `<option value="${d.index}" ${selected}>[${d.index}] ${d.name}${isDef}</option>`;
  });
  sel.innerHTML = html;
}

function populateAudioDeviceDropdowns() {
  const selA = document.getElementById("cfg_audio_dev_a");
  const selB = document.getElementById("cfg_audio_dev_b");
  if (!selA || !selB) return;

  function buildAudioOptions(currentVal, isEnabled) {
    let html = `<option value="DISABLED" ${!isEnabled ? "selected" : ""}>Disabled / No Audio Capture</option>`;
    html += `<option value="" ${isEnabled && (currentVal === null || currentVal === undefined) ? "selected" : ""}>Default System Audio Input</option>`;
    availableAudioDevices.forEach(d => {
      const selected = (isEnabled && currentVal !== null && currentVal !== undefined && parseInt(currentVal, 10) === d.index) ? "selected" : "";
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

function testScanScribeFeeder(scannerId) {
  const targetId = scannerId || 'scanner_a';
  fetch('/api/feeder/dispatch_test', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ scanner_id: targetId })
  })
    .then(res => res.json())
    .then(data => {
      if (data.status === "success" && data.dispatch_result && data.dispatch_result.status === "success") {
        alert(`Dispatched Test Audio Feed [${targetId.toUpperCase()}] to ScanScribe Inbox!\nAudio: ${data.dispatch_result.audio_file}`);
      } else {
        alert(`Feeder Dispatch Result [${targetId.toUpperCase()}]: ${JSON.stringify(data.dispatch_result)}`);
      }
    });
}

window.addEventListener("DOMContentLoaded", () => {
  renderSpecifiersReference();
  connectWebSocket();
  loadModelsCatalog();
  loadAvailableComPorts();
  loadAudioDevices();
  loadAudioOutputDevices(false);
});
