let socket = null;
let currentAppState = null;
let pollingInterval = null;
let availableComPorts = [];
let availableAudioDevices = [];
let availableOutputDevices = [];
let clearedEventTimestamp = 0;

const STANDARD_BAUDS = [115200, 57600, 38400, 19200, 9600, 4800, 2400];

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

function switchSettingsTab(tabKey) {
  document.querySelectorAll(".settings-tab-btn").forEach(btn => {
    btn.classList.toggle("active", btn.dataset.tab === tabKey);
  });
  document.querySelectorAll(".settings-tab-pane").forEach(pane => {
    pane.classList.toggle("active", pane.id === `tabPane_${tabKey}`);
  });
}

function syncSettingsEnablePill(suffix) {
  const chk = document.getElementById(`cfg_enabled_${suffix}`);
  const pill = document.getElementById(`tabPill${suffix.toUpperCase()}`);
  if (!chk || !pill) return;
  const enabled = chk.checked;
  pill.innerText = enabled ? "ON" : "OFF";
  pill.classList.toggle("off", !enabled);
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
        availableComPorts = data.ports || [];
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
    let html = `<option value="NONE" ${currentVal === "NONE" ? "selected" : ""}>NONE (Unassigned)</option>`;
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

  const curA = currentAppState && currentAppState.config ? currentAppState.config.scanner_a.port : "NONE";
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
          availableComPorts.map(p => `- ${p.device} (${p.description})`).join('\n'));
  });
}

function populateBaudDropdown(suffix, selectedBaud) {
  const baudSel = document.getElementById(`cfg_baud_${suffix}`);
  if (!baudSel) return;
  let baudHtml = "";
  STANDARD_BAUDS.forEach(b => {
    baudHtml += `<option value="${b}">${b}</option>`;
  });
  baudSel.innerHTML = baudHtml;
  const val = Number(selectedBaud) || 115200;
  baudSel.value = STANDARD_BAUDS.includes(val) ? String(val) : "115200";
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

function renderUI(state) {
  if (!state) return;

  const scA = state.scanner_a || {};
  const scB = state.scanner_b || {};
  const cfg = state.config || {};
  const events = state.event_log || [];
  const levels = state.audio_levels || { scanner_a: 0, scanner_b: 0 };
  const activeExclusions = state.active_exclusions || {};

  const enabledA = !(cfg.scanner_a && cfg.scanner_a.enabled === false);
  const enabledB = !(cfg.scanner_b && cfg.scanner_b.enabled === false);

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
  const recA = recordingEnabled && enabledA && cfg.scanner_a && cfg.scanner_a.audio_enabled && scA.receiving && !scA.avoided;
  const recB = recordingEnabled && enabledB && cfg.scanner_b && cfg.scanner_b.audio_enabled && scB.receiving && !scB.avoided;
  const globalRecBadge = document.getElementById("globalRecBadge");
  if (globalRecBadge) {
    if (!recordingEnabled) {
      globalRecBadge.className = "rec-badge rec-off";
      globalRecBadge.innerText = "REC DISABLED";
      document.title = "Uniden Scanner Mutual Exclusion Controller";
    } else if (recA || recB) {
      globalRecBadge.className = "rec-badge rec-active";
      globalRecBadge.innerText = "REC ACTIVE";
      document.title = "[REC] Uniden Scanner Mutual Exclusion Controller";
    } else {
      globalRecBadge.className = "rec-badge rec-on";
      globalRecBadge.innerText = "REC STANDBY";
      document.title = "Uniden Scanner Mutual Exclusion Controller";
    }
  }

  // 2. Mutual Exclusion Panel State
  const engineStatusEl = document.getElementById("engineModeStatus");
  if (engineStatusEl) {
    if (enabledA && enabledB) {
      engineStatusEl.className = "engine-status active";
      engineStatusEl.innerText = "Dual-Radio Active";
    } else if (enabledA || enabledB) {
      engineStatusEl.className = "engine-status standby";
      engineStatusEl.innerText = "Single-Radio — Exclusion Standby";
    } else {
      engineStatusEl.className = "engine-status off";
      engineStatusEl.innerText = "All Scanners Disabled";
    }
  }

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
      selectDeviceOptionByNameOrIndex(
        lbOut,
        cfg.audio_loopback.output_device_name,
        cfg.audio_loopback.output_device_index,
        ""
      );
    }
  }

  // 5. Render Scanner A & B Cards
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

function renderLcdMirror(containerEl, displayLines) {
  const key = JSON.stringify(displayLines);
  if (containerEl.dataset.lastLcdKey === key) {
    return;
  }
  containerEl.dataset.lastLcdKey = key;

  const fragment = document.createDocumentFragment();

  for (const line of displayLines) {
    const text = typeof line.text === "string" ? line.text : "";
    const mode = typeof line.mode === "string" ? line.mode : "";

    const rowEl = document.createElement("div");
    rowEl.className = "lcd-row";
    if (line.large) {
      rowEl.classList.add("lcd-row-large");
    }
    if (line.underline_divider) {
      rowEl.classList.add("lcd-row-divider");
    }

    if (!mode) {
      const clean = text.trimEnd();
      rowEl.textContent = clean.length > 0 ? clean : "\u00A0";
    } else {
      let effectiveLen = Math.max(text.length, mode.length);
      while (effectiveLen > 0) {
        const tc = effectiveLen <= text.length ? text[effectiveLen - 1] : " ";
        const mc = effectiveLen <= mode.length ? mode[effectiveLen - 1] : " ";
        if (tc === " " && mc !== "*" && mc !== "#" && mc !== "_") {
          effectiveLen -= 1;
        } else {
          break;
        }
      }

      if (effectiveLen === 0) {
        rowEl.textContent = "\u00A0";
      } else {
        let runText = "";
        let runCls = null;
        const flushRun = () => {
          if (!runText) return;
          if (runCls) {
            const span = document.createElement("span");
            span.className = runCls;
            span.textContent = runText;
            rowEl.appendChild(span);
          } else {
            rowEl.appendChild(document.createTextNode(runText));
          }
          runText = "";
        };

        for (let i = 0; i < effectiveLen; i += 1) {
          const ch = i < text.length ? text[i] : " ";
          const m = i < mode.length ? mode[i] : " ";
          let cls = "";
          if (m === "*") cls = "lcd-inv";
          else if (m === "#") cls = "lcd-inv-ul";
          else if (m === "_") cls = "lcd-ul";

          if (runCls !== null && cls !== runCls) {
            flushRun();
          }
          runCls = cls;
          runText += ch;
        }
        flushRun();
      }
    }

    fragment.appendChild(rowEl);
  }

  containerEl.replaceChildren(fragment);
}

function renderScannerCard(suffix, data, scannerCfg, masterRecordEnabled, audioLevelPct) {
  const card = document.getElementById(`card${suffix}`);
  const badge = document.getElementById(`badge${suffix}`);
  const statusText = document.getElementById(`statusText${suffix}`);
  const recBadge = document.getElementById(`recBadge${suffix}`);
  const modelBadge = document.getElementById(`cardModelBadge${suffix}`);

  const lcdScreen = document.getElementById(`lcdScreen${suffix}`);
  const lcdFallback = document.getElementById(`lcdFallback${suffix}`);
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
  const cardAudioWrap = document.getElementById(`cardAudioWrap${suffix}`);
  const activeBody = document.getElementById(`cardActiveBody${suffix}`);
  const disabledNotice = document.getElementById(`cardDisabledNotice${suffix}`);

  if (!card) return;

  const isDisabled = data.status_line === "DISABLED" || scannerCfg.enabled === false;
  const idleText = isDisabled ? "DISABLED" : (data.connected ? "Scanning..." : "DISCONNECTED");

  if (modelBadge) {
    modelBadge.innerText = isDisabled
      ? "DISABLED"
      : (data.model || (data.connected ? "SCANNER" : "AUTO-DETECT"));
  }

  if (cardEnable && document.activeElement !== cardEnable) {
    cardEnable.checked = scannerCfg.enabled !== false;
  }
  if (cardAudioEnable && document.activeElement !== cardAudioEnable) {
    cardAudioEnable.checked = scannerCfg.audio_enabled !== false;
  }
  if (cardAudioWrap) {
    cardAudioWrap.style.display = isDisabled ? "none" : "";
  }

  if (activeBody) activeBody.style.display = isDisabled ? "none" : "";
  if (disabledNotice) disabledNotice.style.display = isDisabled ? "block" : "none";
  card.style.opacity = isDisabled ? "0.6" : "1";

  const hasLiveLcd = !isDisabled
    && data.connected
    && Array.isArray(data.display_lines)
    && data.display_lines.length > 0;

  if (hasLiveLcd && lcdScreen) {
    lcdScreen.style.display = "flex";
    if (lcdFallback) lcdFallback.style.display = "none";
    renderLcdMirror(lcdScreen, data.display_lines);
  } else {
    if (lcdScreen) {
      lcdScreen.style.display = "none";
      lcdScreen.dataset.lastLcdKey = "";
    }
    if (lcdFallback) lcdFallback.style.display = "";
    sysEl.innerText = data.system_name || idleText;
    deptEl.innerText = data.dept_name || "";
    chanEl.innerText = data.channel_name || idleText;
  }
  tgidEl.innerText = data.tgid || "--";
  freqEl.innerText = data.frequency ? `${data.frequency.toFixed(4)} MHz` : "-- MHz";
  modEl.innerText = data.modulation || "NFM";
  if (uidEl) uidEl.innerText = data.unit_id || "--";

  // Per-card REC Badge
  if (recBadge) {
    const canRecord = masterRecordEnabled && !isDisabled && scannerCfg.audio_enabled !== false;
    if (!canRecord) {
      recBadge.className = "rec-badge rec-off";
      recBadge.innerText = "REC OFF";
    } else if (data.receiving && !data.avoided) {
      recBadge.className = "rec-badge rec-active";
      recBadge.innerText = "REC";
    } else {
      recBadge.className = "rec-badge rec-on";
      recBadge.innerText = "REC ON";
    }
  }

  // Live Audio VU Meter
  const clampedLevel = isDisabled ? 0 : Math.max(0, Math.min(100, Number(audioLevelPct) || 0));
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
  if (isDisabled) {
    card.classList.remove("receiving");
    badge.className = "status-badge scanning";
    statusText.innerText = "DISABLED";
  } else if (data.receiving) {
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
    const level = isDisabled ? 0 : (data.rssi || (data.receiving ? 4 : 0));
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
  if (btn) btn.innerText = "Scanning...";

  if (body) {
    body.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-dim); padding: 20px;">Probing COM ports at standard baud rates... Please wait.</td></tr>`;
  }

  fetch('/api/ports/detect')
    .then(res => res.json())
    .then(data => {
      if (btn) btn.innerText = "Scan Serial Ports";
      if (data.status === "success") {
        renderAutoDetectGrid(data.ports);
      }
    })
    .catch(err => {
      if (btn) btn.innerText = "Scan Serial Ports";
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
      btnHtml = `<span style="font-size: 11px; color: var(--text-dim);">Active / Busy</span>`;
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
  if (detectedModel) target.model = detectedModel;
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
  if (detectedModel) target.model = detectedModel;

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
    updated.audio_loopback = { enabled: true, output_device_index: null, output_device_name: null, volume: 0.8, pan_mode: "STEREO_SPLIT" };
  }

  const lbToggle = document.getElementById("loopbackEnabledToggle");
  const lbOut = document.getElementById("loopbackOutputSelect");
  const lbPan = document.getElementById("loopbackPanSelect");
  const lbVol = document.getElementById("loopbackVolumeSlider");

  if (lbToggle) updated.audio_loopback.enabled = lbToggle.checked;
  if (lbOut) {
    const selOpt = lbOut.options[lbOut.selectedIndex];
    if (lbOut.value === "") {
      updated.audio_loopback.output_device_index = null;
      updated.audio_loopback.output_device_name = null;
    } else if (selOpt && selOpt.dataset.name) {
      updated.audio_loopback.output_device_name = selOpt.dataset.name;
      updated.audio_loopback.output_device_index = lbOut.value.startsWith("NAME:")
        ? null
        : parseInt(lbOut.value, 10);
    }
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
  if (!modal) return;
  modal.classList.add("active");
  switchSettingsTab("scannerA");

  loadAvailableComPorts(() => {
    if (currentAppState && currentAppState.config) {
      const cfg = currentAppState.config;
      const scA = currentAppState.scanner_a || {};
      const scB = currentAppState.scanner_b || {};

      const chkA = document.getElementById("cfg_enabled_a");
      const chkB = document.getElementById("cfg_enabled_b");
      if (chkA) chkA.checked = cfg.scanner_a.enabled !== false;
      if (chkB) chkB.checked = cfg.scanner_b.enabled !== false;
      syncSettingsEnablePill('a');
      syncSettingsEnablePill('b');

      const dmA = document.getElementById("cfg_detected_model_a");
      const dmB = document.getElementById("cfg_detected_model_b");
      if (dmA) {
        dmA.innerText = scA.connected && scA.model
          ? `${scA.model} (Auto-detected)`
          : `${cfg.scanner_a.model || 'Auto-detected on connect'}`;
      }
      if (dmB) {
        dmB.innerText = scB.connected && scB.model
          ? `${scB.model} (Auto-detected)`
          : `${cfg.scanner_b.model || 'Auto-detected on connect'}`;
      }

      document.getElementById("cfg_mode_a").value = cfg.scanner_a.connection_mode;
      if (document.getElementById("cfg_lockout_a")) {
        document.getElementById("cfg_lockout_a").value = cfg.scanner_a.lockout_method || "KEYPRESS";
      }

      document.getElementById("cfg_mode_b").value = cfg.scanner_b.connection_mode;
      if (document.getElementById("cfg_lockout_b")) {
        document.getElementById("cfg_lockout_b").value = cfg.scanner_b.lockout_method || "KEYPRESS";
      }

      populateComPortDropdowns();
      populateBaudDropdown('a', cfg.scanner_a.baud_rate);
      populateBaudDropdown('b', cfg.scanner_b.baud_rate);

      const portA = cfg.scanner_a.connection_mode === "IP"
        ? (cfg.scanner_a.ip_host || "127.0.0.1")
        : cfg.scanner_a.port;
      const portB = cfg.scanner_b.connection_mode === "IP"
        ? (cfg.scanner_b.ip_host || "127.0.0.1")
        : cfg.scanner_b.port;

      const selA = document.getElementById("cfg_port_a");
      const customA = document.getElementById("cfg_custom_port_a");
      if (cfg.scanner_a.connection_mode === "IP") {
        customA.value = portA;
      } else if ([...selA.options].some(o => o.value === portA)) {
        selA.value = portA;
        customA.style.display = "none";
      } else {
        selA.value = "CUSTOM";
        customA.value = portA;
        customA.style.display = "block";
      }

      const selB = document.getElementById("cfg_port_b");
      const customB = document.getElementById("cfg_custom_port_b");
      if (cfg.scanner_b.connection_mode === "IP") {
        customB.value = portB;
      } else if ([...selB.options].some(o => o.value === portB)) {
        selB.value = portB;
        customB.style.display = "none";
      } else {
        selB.value = "CUSTOM";
        customB.value = portB;
        customB.style.display = "block";
      }

      toggleConnectionModeUI('a');
      toggleConnectionModeUI('b');

      document.getElementById("cfg_filename_template_a").value = cfg.scanner_a.filename_template || "%DT - %S - %C (%TG)";
      document.getElementById("cfg_tit2_template_a").value = cfg.scanner_a.tit2_template || "%C (%TG)";
      document.getElementById("cfg_filename_template_b").value = cfg.scanner_b.filename_template || "%DT - %S - %C (%TG)";
      document.getElementById("cfg_tit2_template_b").value = cfg.scanner_b.tit2_template || "%C (%TG)";

      if (cfg.feeder) {
        const outDirEl = document.getElementById("cfg_output_dir");
        if (outDirEl) outDirEl.value = cfg.feeder.inbox_directory || "";
        document.getElementById("cfg_feeder_format").value = cfg.feeder.audio_format || "MP3";
        document.getElementById("cfg_feeder_sample_rate").value = String(cfg.feeder.sample_rate || 16000);
        document.getElementById("cfg_feeder_proscan_tags").checked = cfg.feeder.enable_proscan_tags !== false;
        document.getElementById("cfg_feeder_sidecar_json").checked = cfg.feeder.enable_sidecar_json !== false;
      }

      updateTemplatePreviews();
      loadAudioDevices(false);
    }
  });
}

function closeSettingsModal() {
  const modal = document.getElementById("settingsModal");
  if (modal) modal.classList.remove("active");
}

function getSelectedPortOrHost(suffix) {
  const mode = document.getElementById(`cfg_mode_${suffix}`).value;
  const sel = document.getElementById(`cfg_port_${suffix}`);
  const custom = document.getElementById(`cfg_custom_port_${suffix}`);

  if (mode === "IP") {
    return { isIp: true, value: custom.value.trim() || "127.0.0.1" };
  }
  if (sel.value === "CUSTOM") {
    return { isIp: false, value: custom.value.trim() || "NONE" };
  }
  return { isIp: false, value: sel.value };
}

function extractAudioSelection(selectEl) {
  if (!selectEl) return { enabled: false, index: null, name: null };
  const val = selectEl.value;
  if (val === "DISABLED") {
    return { enabled: false, index: null, name: null };
  }
  if (val === "") {
    return { enabled: true, index: null, name: null };
  }
  const opt = selectEl.options[selectEl.selectedIndex];
  const devName = opt && opt.dataset.name ? opt.dataset.name : null;
  const idx = val.startsWith("NAME:") ? null : parseInt(val, 10);
  return {
    enabled: true,
    index: isNaN(idx) ? null : idx,
    name: devName
  };
}

function saveConfiguration(event) {
  event.preventDefault();

  if (!currentAppState || !currentAppState.config) return;

  const cfg = JSON.parse(JSON.stringify(currentAppState.config));

  cfg.scanner_a.enabled = document.getElementById("cfg_enabled_a").checked;
  cfg.scanner_a.connection_mode = document.getElementById("cfg_mode_a").value;
  const epA = getSelectedPortOrHost('a');
  if (epA.isIp) {
    cfg.scanner_a.ip_host = epA.value;
  } else {
    cfg.scanner_a.port = epA.value;
  }
  cfg.scanner_a.baud_rate = parseInt(document.getElementById("cfg_baud_a").value, 10);
  cfg.scanner_a.lockout_method = document.getElementById("cfg_lockout_a").value;

  cfg.scanner_b.enabled = document.getElementById("cfg_enabled_b").checked;
  cfg.scanner_b.connection_mode = document.getElementById("cfg_mode_b").value;
  const epB = getSelectedPortOrHost('b');
  if (epB.isIp) {
    cfg.scanner_b.ip_host = epB.value;
  } else {
    cfg.scanner_b.port = epB.value;
  }
  cfg.scanner_b.baud_rate = parseInt(document.getElementById("cfg_baud_b").value, 10);
  cfg.scanner_b.lockout_method = document.getElementById("cfg_lockout_b").value;

  // Audio Capture Selections (Name-first)
  const audA = extractAudioSelection(document.getElementById("cfg_audio_dev_a"));
  cfg.scanner_a.audio_enabled = audA.enabled;
  cfg.scanner_a.audio_device_index = audA.index;
  cfg.scanner_a.audio_device_name = audA.name;
  cfg.scanner_a.audio_channel = document.getElementById("cfg_audio_ch_a").value;

  const audB = extractAudioSelection(document.getElementById("cfg_audio_dev_b"));
  cfg.scanner_b.audio_enabled = audB.enabled;
  cfg.scanner_b.audio_device_index = audB.index;
  cfg.scanner_b.audio_device_name = audB.name;
  cfg.scanner_b.audio_channel = document.getElementById("cfg_audio_ch_b").value;

  // Templates
  cfg.scanner_a.filename_template = document.getElementById("cfg_filename_template_a").value.trim() || "%DT - %S - %C (%TG)";
  cfg.scanner_a.tit2_template = document.getElementById("cfg_tit2_template_a").value.trim() || "%C (%TG)";
  cfg.scanner_b.filename_template = document.getElementById("cfg_filename_template_b").value.trim() || "%DT - %S - %C (%TG)";
  cfg.scanner_b.tit2_template = document.getElementById("cfg_tit2_template_b").value.trim() || "%C (%TG)";

  if (cfg.feeder) {
    cfg.feeder.feeder_mode = "DIRECTORY_DROP";
    cfg.feeder.audio_format = document.getElementById("cfg_feeder_format").value;
    cfg.feeder.sample_rate = parseInt(document.getElementById("cfg_feeder_sample_rate").value, 10);
    const outDirEl = document.getElementById("cfg_output_dir");
    if (outDirEl) {
      cfg.feeder.inbox_directory = outDirEl.value.trim();
    }
    cfg.feeder.enable_proscan_tags = document.getElementById("cfg_feeder_proscan_tags").checked;
    cfg.feeder.enable_sidecar_json = document.getElementById("cfg_feeder_sidecar_json").checked;
  }

  fetch('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cfg)
  }).then(() => {
    closeSettingsModal();
  });
}

function loadAudioDevices(forceRefresh) {
  const qs = forceRefresh ? "?refresh=true" : "";
  fetch(`/api/audio/devices${qs}`)
    .then(res => res.json())
    .then(data => {
      if (data.status === "success") {
        availableAudioDevices = data.devices || [];
        populateAudioDeviceDropdowns();
      }
    })
    .catch(err => console.error("Failed to load audio devices:", err));
}

function loadAudioOutputDevices(forceRefresh) {
  const qs = forceRefresh ? "?refresh=true" : "";
  fetch(`/api/audio/output_devices${qs}`)
    .then(res => res.json())
    .then(data => {
      if (data.status === "success") {
        availableOutputDevices = data.devices || [];
        populateAudioOutputDropdown();
        if (forceRefresh) {
          alert(`Refreshed output speakers!\nFound ${availableOutputDevices.length} output device(s).`);
        }
      }
    })
    .catch(err => console.error("Failed to load audio output devices:", err));
}

function escapeHtmlAttr(str) {
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function selectDeviceOptionByNameOrIndex(sel, devName, devIdx, defaultVal) {
  if (!sel) return;
  const opts = [...sel.options];
  if (devName) {
    const byName = opts.find(o => o.dataset.name === devName);
    if (byName) {
      sel.value = byName.value;
      return;
    }
  }
  if (devIdx !== null && devIdx !== undefined) {
    const idxStr = String(devIdx);
    if (opts.some(o => o.value === idxStr)) {
      sel.value = idxStr;
      return;
    }
  }
  if (defaultVal !== undefined && opts.some(o => o.value === defaultVal)) {
    sel.value = defaultVal;
  }
}

function populateAudioOutputDropdown() {
  const sel = document.getElementById("loopbackOutputSelect");
  if (!sel) return;

  const lbCfg = (currentAppState && currentAppState.config && currentAppState.config.audio_loopback) || {};
  const curName = lbCfg.output_device_name || null;
  const curIdx = lbCfg.output_device_index;

  let html = `<option value="">Default System Output Speaker</option>`;
  let matchedByName = false;
  availableOutputDevices.forEach(d => {
    const isDef = d.is_default ? " (System Default)" : "";
    if (curName && d.name === curName) matchedByName = true;
    html += `<option value="${d.index}" data-name="${escapeHtmlAttr(d.name)}">[${d.index}] ${escapeHtmlAttr(d.name)}${isDef}</option>`;
  });
  if (curName && !matchedByName) {
    html += `<option value="NAME:${escapeHtmlAttr(curName)}" data-name="${escapeHtmlAttr(curName)}" selected>${escapeHtmlAttr(curName)} (Configured)</option>`;
  }
  sel.innerHTML = html;
  selectDeviceOptionByNameOrIndex(sel, curName, curIdx, "");
}

function populateAudioDeviceDropdowns() {
  const selA = document.getElementById("cfg_audio_dev_a");
  const selB = document.getElementById("cfg_audio_dev_b");
  if (!selA || !selB) return;

  function buildAudioOptions(curName, curIdx, isEnabled) {
    let html = `<option value="DISABLED" ${!isEnabled ? "selected" : ""}>Disabled / No Audio Capture</option>`;
    html += `<option value="">Default System Audio Input</option>`;
    let matchedByName = false;
    availableAudioDevices.forEach(d => {
      const isDef = d.is_default ? " (System Default)" : "";
      if (curName && d.name === curName) matchedByName = true;
      html += `<option value="${d.index}" data-name="${escapeHtmlAttr(d.name)}">${escapeHtmlAttr(d.name)} (${d.channels} ch)${isDef}</option>`;
    });
    if (curName && !matchedByName) {
      html += `<option value="NAME:${escapeHtmlAttr(curName)}" data-name="${escapeHtmlAttr(curName)}">${escapeHtmlAttr(curName)} (Configured)</option>`;
    }
    return html;
  }

  const cfgA = (currentAppState && currentAppState.config && currentAppState.config.scanner_a) || {};
  const cfgB = (currentAppState && currentAppState.config && currentAppState.config.scanner_b) || {};

  selA.innerHTML = buildAudioOptions(cfgA.audio_device_name, cfgA.audio_device_index, cfgA.audio_enabled !== false);
  selB.innerHTML = buildAudioOptions(cfgB.audio_device_name, cfgB.audio_device_index, cfgB.audio_enabled !== false);

  if (cfgA.audio_enabled === false) {
    selA.value = "DISABLED";
  } else {
    selectDeviceOptionByNameOrIndex(selA, cfgA.audio_device_name, cfgA.audio_device_index, "");
  }

  if (cfgB.audio_enabled === false) {
    selB.value = "DISABLED";
  } else {
    selectDeviceOptionByNameOrIndex(selB, cfgB.audio_device_name, cfgB.audio_device_index, "");
  }

  const chA = document.getElementById("cfg_audio_ch_a");
  const chB = document.getElementById("cfg_audio_ch_b");
  if (chA && cfgA.audio_channel) chA.value = cfgA.audio_channel;
  if (chB && cfgB.audio_channel) chB.value = cfgB.audio_channel;
}

window.addEventListener("DOMContentLoaded", () => {
  renderSpecifiersReference();
  connectWebSocket();
  loadAvailableComPorts();
  loadAudioDevices(false);
  loadAudioOutputDevices(false);
});
