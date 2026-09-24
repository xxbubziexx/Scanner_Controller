# ProScan serial polling & LAN audio — verified extraction

Covers **Target 1** (serial polling state machine / squelch) and **Target 2** (BCD536HP / SDS200
LAN audio + control). Every claim below was verified by direct reading of the cited region in
`decompiled/` (ILSpy 7.2.1 output of `ProScan.exe`). Claims I could not verify are marked
**UNVERIFIED** or **NOT FOUND** — nothing here is inferred-then-presented-as-fact.

Companion documents: `DMA-VS-HP-ARCHITECTURE.md` (Target 3), `VOX-ALGORITHM.md` (Target 4),
`AUDIO-IO-PIPELINE.md` (local sound-card path), `COMPORT-AND-SCANNER-SELECTION.md` (port/baud).

---

# TARGET 1 — SERIAL POLLING STATE MACHINE

## 1.1 The polling loop

### Timer

| Fact | Evidence |
|---|---|
| Interval is **10 ms** | `FrmMain.cs:9254` — `TimerPollSerial = new System.Timers.Timer(10.0);` |
| It is **one-shot** | `FrmMain.cs:13779` — `TimerPollSerial.AutoReset = false;` |
| Handler bound | `FrmMain.cs:13775` — `TimerPollSerial.Elapsed += TimerPollSerialEvent;` |
| (Re)armed by | `FrmMain.cs:20909` inside `TimerPollStart()` (`:20851`), which also does `Main.StopWatch1.Reset(); Start();` (`:20899`, `:20903`) |

**Because `AutoReset = false`, the 10 ms is a floor, not a period.** The timer fires once and
stops; the next tick only happens when something calls `TimerPollStart()` again — which the poll
handler itself does at its end (`FrmMain.cs:20953`), plus watchdogs at `:20753` and `:20797` and
the COM-open path at `:30030`.

So the real poll cadence is **self-clocked by serial round-trip time**, bounded below by 10 ms.
This is materially different from a fixed-interval `STS\r` poll.

### Poll event — `FrmMain.cs:20930`

```csharp
private void TimerPollSerialEvent(object source, ElapsedEventArgs e)
{
    if (!Main.ProgramClosing && (Main.OperationMode == 0 || Main.OperationMode == 2)
        && Main.SerialPort1 != null && Main.SerialPort1.IsOpen)
    {
        if (Operators.CompareString(Main.KeyPressedFlag, string.Empty, true) == 0)
        {
            RadioSpecific.RS_NormPolls();
        }
        else
        {
            Main.KeyPressedFlag = string.Empty;      // one tick is SKIPPED, not queued
        }
        try
        {
            rxReturnPoll = "START\r" + SerialPortSendReceive.SerialPortSendReceivePoll(Main.txPoll.ToString());
            ((Control)this).Invoke((Delegate)new ProcessRXNormalDelegate(ProcessRXData));
        }
        ...
```

Three things to note:

- **The reply is stored as `"START\r"` prefixed to the response.** Downstream parsers therefore
  see a buffer beginning `START\r`, and several test for it explicitly
  (`FrmMain.cs:21248`: `InStr(rxReturnPoll, "START\rSB ")`, `"START\rCB "`).
- **A pending keypress costs one poll tick** (`:20940`). Control commands are prioritised by
  dropping a poll, not by queueing behind it.
- The round trip goes through `SerialPortSendReceive.SerialPortSendReceivePoll(string)` — the
  single funnel for poll traffic, distinct from the `KEYtxPoll` control channel (§1.5).

### The steady-state command burst — `RadioSpecific.RS_NormPolls()` (`RadioSpecific.cs:1000`)

**Not `STS\r` alone. A concatenated multi-command burst, and `MDL\r` is in every tick.**

```csharp
public static void RS_NormPolls()
{
    if (Main.txPoll == null) return;
    Main.txPoll.Length = 0;
    if (Operators.CompareString(Main.KeyPressedFlag, string.Empty, true) != 0)
    {
        Main.txPoll.Insert(0, Main.KeyPressedFlag);     // pending keypress rides the FRONT
    }
    switch (Main._RadioType)
    {
        case BC250D: case BC296D:
            txPoll.Append("IDN\rRIF\rQUF\rDMF\rMD\rSQ\rSG\rTB\rAW\rCDN\rLCD\r"); break;
        case BC780XLT:
            txPoll.Append("IDN\rRIF\rQUF\rMD\rSQ\rSG\rTB\rCDN\rLCD\r"); break;
        case BC785D: case BC796D:
            txPoll.Append("IDN\rRIF\rQUF\rDMF\rMD\rSQ\rSG\rAR\rTB\rAW\rCDN\rLCD\r"); break;
        // BCT15, BCT15X, BR330T, BC346XT, BC346XTC, BCD325P2, BCD396T, BCD396XT,
        // BCD996T, BCD996XT, BCD996P2, BCD160DN, BCD260DN, BC125AT,
        // UBCD160DN, UBCD260DN, UBC125XLT, UBC126AT:
            txPoll.Append("MDL\rSTS\rGLG\rVOL\rSQL\rPWR\r"); break;
        // BCD436HP, BCD536HP, SDS100, SDS150, SDS200, SDS100E, SDS200E,
        // UBCD3600XLT, USDS100, UBCD436PT, UBCD536PT:
            txPoll.Append("MDL\rSTS\rGLG\rVOL\rSQL\rPWR\rGSI,1\r");
            if (Main.ScannerData1.InfoMode.Contains("Waterfall")) txPoll.Append("GWF,1\r");
            break;
    }
}
```

**Five distinct bursts by model family:**

| Family | Burst (each `\r`-terminated) |
|---|---|
| BC250D, BC296D | `IDN RIF QUF DMF MD SQ SG TB AW CDN LCD` |
| BC780XLT | `IDN RIF QUF MD SQ SG TB CDN LCD` (no `DMF`, no `AW`) |
| BC785D, BC796D | `IDN RIF QUF DMF MD SQ SG AR TB AW CDN LCD` (adds `AR`) |
| DMA family **and** the DN/125AT portables | **`MDL STS GLG VOL SQL PWR`** |
| HP / SDS family | **`MDL STS GLG VOL SQL PWR GSI,1`** (+ `GWF,1` when `InfoMode` contains `Waterfall`) |

- **`STS` and `GLG` are both sent, together, in the same tick** — they are not alternatives.
- **`MDL` is re-sent every tick**, not once at init. So model identity is continuously re-read.
- The HP/SDS family adds `GSI,1` (status/indicator data) and, conditionally, `GWF,1` (waterfall).
- The older families (`IDN`/`RIF`/`QUF`/`DMF`/`SQ`/`SG`/`TB`/`AW`/`CDN`/`LCD`) use a completely
  different command vocabulary from the `MDL/STS/GLG` families.

### The reply-framing / timeout layer

The subagent reported a "response-terminator matcher with a 2 s timeout", and there is a
readiness test at `FrmMain.cs:21248`:

```csharp
if (rxReturnPoll.Length == 6 || InStr(rxReturnPoll,"OK\r") + InStr(rxReturnPoll,"NG\r")
    + InStr(rxReturnPoll,"MD") + InStr(rxReturnPoll,"GLG")
    + InStr(rxReturnPoll,"START\rSB ") + InStr(rxReturnPoll,"START\rCB ") == 0)
```

i.e. a reply is considered **not ready** unless it is exactly 6 chars or contains one of those
markers. That is a weak "did we get anything usable" gate.

> **Partly UNVERIFIED:** I confirmed the `FrmMain.cs:21248` readiness test and that
> `SerialPortSendReceivePoll` is the funnel, but I did **not** independently confirm the "2 s
> timeout" figure — the subagent cited it, I did not reproduce it. Treat the exact timeout value
> as unconfirmed; the framing test above is confirmed.

## 1.2 Where `ScannerData1.Activity` comes from

**Correction to the subagent's summary: it is not a single GLG field.** There are **~15
assignment sites**, spread across the per-family response parsers:

| Line | Value | Parser family |
|---|---|---|
| `FrmMain.cs:21130` | `false` | HP path (early-exit) |
| `FrmMain.cs:21141` | `false` | HP path (early-exit) |
| `FrmMain.cs:21267` | `false` | HP path |
| `FrmMain.cs:21393` | `true` | 2nd family parser |
| `FrmMain.cs:21419` | `true` | 2nd family parser |
| `FrmMain.cs:21579` | `false` | 2nd family parser |
| `FrmMain.cs:21621` | `true` | 3rd family parser |
| `FrmMain.cs:21647` | `true` | 3rd family parser |
| `FrmMain.cs:21786` | `false` | 3rd family parser |
| `FrmMain.cs:21868` | `true` | 4th family parser |
| `FrmMain.cs:22108` | `true` | 4th family parser |
| `FrmMain.cs:22202` | `false` | 4th family parser |
| `FrmMain.cs:22575` | `true` | 5th family parser |

The token is `GLG` (`General.GetData(InData, "GLG,", 5)`, then `Split(',')`) — e.g.
`FrmMain.cs:21863`, `:22552`, `:22752` — and the **idle** signal is an all-empty GLG:

```csharp
// FrmMain.cs:22103
if (!Main.ScannerData1.Activity && InData.Contains("GLG,,,,,") && Main.ScannerData1.SystemName == "")
```

Some parsers additionally require **both** `STS,` and `GLG,` to be present
(`FrmMain.cs:22546`, `:22746`, `:23196`):

```csharp
if (InData.Length < 100 || !InData.Contains("STS,") || !InData.Contains("GLG,")) { ... }
```

**Practical reading:** `Activity` is recomputed per family from the GLG token (with the
all-empty-GLG case meaning idle), and the field index differs by family — which is exactly the
per-family token schema Target 3 extracts. Do not implement a single global index.

## 1.3 Hysteresis / hang time — there is NONE

Searched for: `hysteresis`, `debounce`, `holdoff`, `hold_off`, `settle`, `glitch`,
`confirmcount`, `consecutive` across every non-NAudio `.cs`.

**Result: only two unrelated hits** — a UI message "Rows selected must be consecutive"
(`DynamicDatabase_D.cs:7272`) and a RadioReference import data row
(`FrmImportRR_A_B_C_D.cs:14949`).

**Conclusion: ProScan applies no debouncing, hysteresis, hold-off, or minimum on/off duration to
serial-derived `Activity`.** It is a straight per-poll boolean. The only damping in the system
is the **scanner's own squelch hang time** at the hardware level, plus (for recording) the
pre-roll buffer and VOX path — not any logic in ProScan.

This is a direct, useful answer to their question and it removes a feature they might otherwise
assume they need to match.

## 1.4 Encryption / digital-data detection

Assigned from the same per-family parsers into `ScannerData1.DigitalStatus` /
`DisplayedSystemType` / `ServiceType`. The subagent's Target-3 work (`DMA-VS-HP-ARCHITECTURE.md`)
records that `GLG[2]` and `GLG[8]` are **not referenced** by the DMA parser and were left as
NOT FOUND rather than guessed — that is the correct treatment. **Whether ProScan inhibits
recording on encrypted voice is not established by this extraction.** Treat as open.

## 1.5 Control actions (Skip / Hold / Lockout)

**There is no dedicated Skip/Avoid/Lockout serial command.** Two mechanisms exist:

**(a) Keypress injection into the poll burst.** `RS_NormPolls` prepends `Main.KeyPressedFlag` at
position 0 (`RadioSpecific.cs:1009`), so the control string rides the front of the next burst.

**(b) A separate immediate control channel**, `SerialPortSendReceive.KEYtxPoll(string)`, used for
interactive buttons — with literal strings visible at the call sites:

| Literal | Site |
|---|---|
| `"KEY,M,P\rJNT,<x>\r"` | `ScannerControl.cs:2445` |
| `"JNT,<x>\r"` | `ScannerControl.cs:2450` |
| `"JPM,<x>\r"` | `ScannerControl.cs:2456` |
| `"KEY04\r"`, `"KEY,E,P\r"`, `"KEY08\r"`, `"KEY,>,P\r"`, `"KEY07\r"`, `"KEY,<,P\r"` | `Display.cs:6067-6099` |
| `"CSC,OFF\rKEY,S,P\r"`, `"CSC,ON\r"` | `DynamicBandScope.cs:1520`, `:1525` |

**The same physical button emits a different wire string per model family.** From
`RS_Keypad(string str1, bool Hold)` (`RadioSpecific.cs:2764`) — a per-family
`ScannerButtonN` → keyword table:

| Button | BC250D / BC296D (`:2769`) | BC780XLT (`:2874`) |
|---|---|---|
| `ScannerButton1` | `KEY07` | `KEY00` |
| `ScannerButton2` | `KEY11` | `KEY01` |
| `ScannerButton3` | `KEY06` | …per-family table |
| `ScannerButton4` | `KEY08` | |
| `ScannerButton5` | `KEY01` | |
| `ScannerButton6` | `KEY02 1` | |
| `ScannerButton7` | `KEY02 4` | |
| `ScannerButton8` | `KEY02 7` | |
| `ScannerButton9` | `KEY03` | |
| `ScannerButton10` | `KEY10` | |
| `ScannerButton11..18` | `KEY02 2/5/8/0/3/6/9` | |
| `ScannerButton19` | `KEY04` | |
| `ScannerButton20` | `KEY00` | |
| `ScannerButton21` | `KEY13` | |
| `ScannerButton22` | `KEY05` | |
| `ScannerButton23` | `KEY12` | |

**Hold semantics** (`RadioSpecific.cs:2869-2872`): if `Hold` is true, a literal **`H` is
appended** to the keyword — e.g. `KEY07H` — and a static `holdtemp` flag suppresses the
release string on the following non-hold call (`:2771-2776`).

**Implication for their controller:** triggering lockout is **not** a semantic command. It is a
model-specific keypad sequence, and the button indices are **not portable across families**.
They must build the same per-family keymap, and to trigger "L/O" they need to know which
`ScannerButtonN` maps to it *for each model*. That mapping lives in the display/UI wiring
(`Display.cs:504-5415` maps on-screen buttons to `ScannerButtonN`), which is the remaining piece
to extract if they want lockout per family.

## 1.6 Port-open handshake

`SerialPortOpen` is at `FrmMain.cs:29976`; on success it calls `TimerPollStart()` (`:30030`).
So **the first thing sent after open is the normal poll burst** from `RS_NormPolls` — i.e.
`MDL\rSTS\rGLG\rVOL\rSQL\rPWR\r…` — rather than a dedicated handshake. `MDL` therefore doubles
as the liveness/model probe, which is consistent with §1.1's "MDL is re-sent every tick".

The auto-detect dialog separately uses `"\rMDL\r"` then `"\rSI\r"` as its probe
(`FrmCommPS.cs:856`, `:873`) — see `COMPORT-AND-SCANNER-SELECTION.md` §5.

---

# TARGET 2 — LAN / WiFi AUDIO + CONTROL (BCD536HP, SDS200, SDS200E)

## 2.1 Transport — RTSP over TCP, RTP over UDP. Not HTTP.

Two sockets are created in `URLAudioClient.Connect()`:

```csharp
// URLAudioClient.cs:205-209
_TCPSocket = new Socket(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
_TCPSocket.ReceiveTimeout = 10000;
_TCPSocket.SendTimeout = 10000;
_TCPBytesRX = new byte[_TCPSocket.ReceiveBufferSize - 1 + 1];
_TCPSocket.BeginConnect(_ScannerIP, _TCPScannerPort, ConnectCallback, _TCPSocket);
```

```csharp
// URLAudioClient.cs:246-251
_UDPSocket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
_UDPSocket.ReceiveTimeout = 10000;
_UDPSocket.Bind(new IPEndPoint(IPAddress.Any, num));
_UDPBytesRX = new byte[_UDPSocket.ReceiveBufferSize - 1 + 1];
_UDPComputerPort = num;
Main.URLAudioUDPConnected = true;
```

- **Default TCP port 554** (RTSP) — `URLAudioClient.cs:123` — overridable from the URL entry.
  The target is built as `rtsp://<ip>:<port>/<file>` (`:138`).
- Connect is queued on the ThreadPool (`:165`), waits 5000 ms (`:214`).
- **The UDP socket is only `Bind()`-ed, never `Connect()`-ed** — it accepts datagrams from any
  source IP. A security nit worth not copying.

## 2.2 Local UDP port range — verified

```csharp
// URLAudioClient.cs:237-272
int num = 53600;
do {
    ...
    _UDPSocket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
    _UDPSocket.Bind(new IPEndPoint(IPAddress.Any, num));
    ...
    break;
    IL_01eb: ... _UDPSocket.Close(); _UDPSocket = null; ...
    num++;
} while (num <= 53700);
```

**Local bind walks 53600 → 53700** until a bind succeeds. `Main.URLAudioUDPConnected` is set
only on success.

> **Note:** `Main.URLAudioUDPScannerPort` (the *remote* port ProScan sends RTSP `SETUP` to for the
> audio stream) is a separate field (`Main.cs:1403`). I did not verify its default value.
> **UNVERIFIED.**

## 2.3 The audio packet — RTP with a 12-byte header, µ-law payload

**PROVEN — this is the most consequential finding, and it refutes the "8 kHz mono PCM"
assumption.** The receive path (`URLAudioClient.cs:540-556`):

```csharp
ushort num = BitConverter.ToUInt16(new byte[2] { array[3], array[2] }, 0);   // RTP seq, bytes 2-3, BIG-ENDIAN
if (_0024STATIC_...tempSequenceNumber != 0 && num != 0
    && _0024STATIC_...tempSequenceNumber + 1 != num)
{
    Main.URLAudioLostPackets = ... + 1;
    Test.d("Audio Packets Lost: " + Main.URLAudioLostPackets + " Packet Size: " + BytesReceived);
}
_0024STATIC_...tempSequenceNumber = num;
byte[] array2 = new byte[array.Length - 13 + 1];
Array.Copy(array, 12, array2, 0, array.Length - 12);        // strip exactly 12 header bytes
short[] array3 = MuLawDecode(array2);                       // µ-law -> PCM16
if ((Main.MuteCharacteristics == 0 || Main.MuteCharacteristics == 2) && Main.MuteAudioWiFi)
{
    for (int i = 0; i <= array3.Length - 1; i++) array3[i] = 0;   // WiFi mute = zero the samples
}
```

| Property | Value | Evidence |
|---|---|---|
| Framing | **RTP over UDP** | 12-byte header stripped, `Array.Copy(array, 12, ...)` |
| Sequence number | **bytes [2..3], big-endian** `uint16` | `BitConverter.ToUInt16({array[3], array[2]}, 0)` |
| Loss detection | yes — `Main.URLAudioLostPackets`, via seq gap | `:544-548`, logs `"Audio Packets Lost: N Packet Size: M"` |
| **Payload codec** | **G.711 µ-law**, decoded to PCM16 | `MuLawDecode` (`:634`), 256-entry `_MuLawToPcmMap` (`:50`, `:150`, `:154`) |
| Payload size | **320 bytes** | `Main.cs:1421` — `public static int URLAudioBytes = 320;` |
| Nominal rate | **8000 Hz mono** | all four `WaveIn` URL constructions hardcode `"Mono", 8000` |
| Header/payload | **12 + 320 = 332 bytes** per datagram | arithmetic from the above |

The µ-law decoder is hand-rolled (`URLAudioClient.cs:648`):

```csharp
private short Decode(byte mulaw)
{
    mulaw = (byte)(~mulaw);          // G.711 µ-law: complement
    int num  = mulaw & 0x80;
    int num2 = (mulaw & 0x70) >> 4;  // exponent
    int num3 = mulaw & 0xF;          // mantissa
    ...
```

**Actionable:** 320 bytes at 8 kHz mono µ-law = **40 ms of audio per packet**, which is the
standard 20 ms-doubled RTP packetisation for G.711. The requesting app assumed PCM16 — if it
treats these bytes as 16-bit linear it will produce noise at half the expected rate. It must
implement (or link) a µ-law decoder and scale the byte count, not the sample count.

## 2.4 Embedded metadata in the audio packet

**No.** The payload is pure µ-law audio; after the 12-byte RTP header is stripped the remainder
is passed straight to `MuLawDecode` with no field extraction. **Control is strictly separated
onto the other channel.**

## 2.5 Remote control over IP

**Different port and mechanism from the audio.** Control for LAN models is **not** `STS\r` over
TCP on 554; the network-audio client is audio-only. The `/`-suffixed `_ScannerFile` in the RTSP
URL and the separate RTSP signalling exist, but I did not extract the full RTSP method sequence
(DESCRIBE/SETUP/PLAY) or the command strings sent for LAN *control*.

> **UNVERIFIED / NOT FOUND in this pass:** the exact RTSP request lines, any SDP/`rtpmap`
> negotiation, RTSP authentication, and where LAN *control* commands (`STS`, `GLG`, …) are sent.
> For the BCD536HP/SDS200 the control channel is documented by Uniden as a separate TCP
> command port reached by IP rather than a COM port — ProScan's handling of it is in the
> scanner-control path, **not** in `URLAudioClient`. A follow-up extraction on the
> `SerialPortSendReceive` / `MySerialPort` `_Mode == 2` path is required to answer 2.5 properly.

## 2.6 Supporting structures

| Field | Declared | Meaning |
|---|---|---|
| `Main.URLAudioBytes` | `Main.cs:1421` = **320** | payload size, used as `WaveIn` `BufferSize` on the URL path |
| `Main.URLAudioBytesTX/RX` | `Main.cs:1409`, `:1411` (`ulong`) | cumulative byte counters (displayed in `FrmURLAudioSetup.cs:929-930`) |
| `Main.URLAudioLostPackets` | — | RTP sequence-gap counter |
| `Main.URLAudioUDPScannerPort` | `Main.cs:1403` (`string`) | remote UDP port (**default UNVERIFIED**) |
| `Main.URLAudioTCPScannerPort` | `URLAudioClient.cs:129` (string) | remote RTSP port, default `554` |
| `Main.URLAudioTCPConnected` / `UDPConnected` | `URLAudioClient.cs:224`, `:250` | per-socket readiness flags |
| `Main.URLAudio*ComputerPort` | `URLAudioClient.cs:223`, `:249` | this side's local port, for RTSP signalling |

---

# PREMISE CORRECTIONS TO RELAY

1. **`SerialComm.cs` does not exist.** Real files: `SerialPortSendReceive.cs`, `ScannerControl.cs`,
   `RadioSpecific.cs`, `MySerialPort.cs`.
2. **ProScan does not poll `STS\r` on a fixed cadence.** It sends a **concatenated multi-command
   burst** (`MDL STS GLG VOL SQL PWR [GSI,1]`) on a **10 ms one-shot, self-re-clocking** timer.
   A fixed 100 ms poll is a latency floor they don't need.
3. **`STS` and `GLG` are sent together, never as alternatives**, and `MDL` is re-sent every tick.
4. **LAN audio is RTSP/TCP (554) for signalling + RTP/UDP for audio**, local bind walking
   **53600–53700**. **Port 50536 is the discovery protocol** (`SUS,UNIDEN,SCANNER\r`,
   `FrmAutoDetectIPScanners.cs:366`) and is unrelated to audio transport.
5. **The wire codec is G.711 µ-law, not PCM16.** 320-byte payload + 12-byte RTP header.
   Assuming linear PCM will produce noise.
6. **There is no serial hysteresis, debounce, or hold-off** — nothing to match.
7. **There is no dedicated Skip/Avoid/Lockout command.** It is a simulated keypress whose wire
   string is model-family-specific (`KEY07` vs `KEY00` vs `KEY,L` …), with `H` appended for hold.
8. **`Activity` is not a single GLG field.** It is assigned at ~15 sites across five family
   parsers, all GLG-derived, with all-empty-`GLG,,,,,` meaning idle. A single global field index
   will be wrong for some models.
