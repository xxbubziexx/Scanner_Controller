# ProScan COM port + scanner selection — how it works

Reverse-engineered from `ProScan.exe` (VB.NET, .NET 4.8, unobfuscated). Every claim carries
its provenance as `file:line` in `decompiled/` (ILSpy 7.2.1 output).

Source files:
- `FrmRadioType.cs` — the **Scanner Type** dialog
- `FrmCommPS.cs` — the **Comm Port Setup** dialog (port, baud, auto-detect)
- `Common_PS_RF.cs` — port enumeration
- `RadioSpecific.cs` — per-model tables and behaviour
- `FrmMain.cs` — menu handlers, `SerialPortOpen`/`SerialPortClose`
- `MySerialPort.cs` — the serial/UDP transport wrapper
- `FrmAutoDetectIPScanners.cs` — LAN discovery of networked scanners
- `Main.cs`, `General.cs` — state variables and `ProScan.cfg` persistence

---

## 0. The two menu items

Both live on the same menu, both gated by `Allow(3)`:

| Menu item | Handler | Opens |
|---|---|---|
| **Scanner Type** | `FrmMain.cs:34670` | `FrmRadioType` modal dialog |
| **Comm Port** | `FrmMain.cs:34689` | `FrmCommPS` modal dialog |

They are **independent**. The scanner type is a separate dialog from the port dialog, and the
only coupling is that the port dialog's port list is *conditional on the currently selected
scanner type* (§3) and its "Use Selected" button can silently change the scanner type (§6).

---

## 1. Scanner selection — `FrmRadioType`

### The list is an enum. Nothing more.

`FrmRadioType_Load` (`FrmRadioType.cs:145`):

```csharp
List<string> list = new List<string>();
string[] names = Enum.GetNames(typeof(Main.RadioType));
foreach (string item in names) { if (0 == 0) { list.Add(item); } }
list.Sort();
ComboBox1.Items.AddRange(list.ToArray());
ComboBox1.Text = Main._RadioType.ToString();
ComboBox1.DropDownHeight = ComboBox1.Items.Count * (ComboBox1.ItemHeight + 2);
```

So the menu is `Enum.GetNames(Main.RadioType)`, **alphabetically sorted** (which is why the
on-screen order is not the enum's declaration order). There is no data file, no plugin
registry, no capability list — adding a model means adding an enum member and recompiling.

The `if (0 == 0)` is a decompiler artifact of a compiler-generated filter; it filters nothing.

### The model list — `Main.RadioType` (`Main.cs:199`), 34 members

```
BCT15        BCT15X      BC250D      BC296D      BR330T      BC346XT
BC346XTC     BCD325P2    BCD396T     BCD396XT    BCD436HP    BCD536HP
BC780XLT     BC785D      BC796D      BCD996T     BCD996XT    BCD996P2
SDS100       SDS150      SDS200      SDS100E     SDS200E     UBCD3600XLT
USDS100      BCD160DN    BCD260DN    BC125AT     UBCD160DN   UBCD260DN
UBCD436PT    UBCD536PT   UBC125XLT   UBC126AT
```

All Uniden/Bearcat, consumer + EMEA (`*E`, `UBCD*`, `UBC*`) variants. No Whistler, no GRE,
no RadioShack.

### Apply flow

`Button1_Click` (`FrmRadioType.cs:163`) stashes the selection on the form's `Tag` and closes —
**the dialog itself changes nothing**. The caller does the work (`FrmMain.cs:34670`):

```csharp
string text = frmRadioType.Tag;
if (text != "" && text != Main._RadioType.ToString() && ClosingDB("5"))
{
    Main._RadioType = (Main.RadioType)Enum.Parse(typeof(Main.RadioType), text);
    RadioSpecific.RS_InitRadioType();
}
```

Three gates before the change lands:
1. non-empty selection,
2. actually *different* from the current type (re-picking the same model is a no-op),
3. **`ClosingDB("5")`** — a confirm prompt. The DB/programming data is model-specific, so
   switching models mid-session offers to close the open database first. If the user declines,
   the type change is abandoned entirely.

`RadioSpecific.RS_InitRadioType()` then rebuilds all the per-model tables.

---

## 2. Comm Port dialog — `FrmCommPS` layout

```
ComboBox1   port list        ("None", "1", "2", …, optionally "URL")
ComboBox2   baud list        (model-dependent, from RS_AddBaudData)
DataGridView15  4 columns:  Port | Status | Scanner | Scanner Baud Rate
```

Buttons (`FrmCommPS.cs`):

| Button | Handler | Action |
|---|---|---|
| Refresh | `:668` | re-enumerate ports, preserve current text |
| (apply / OK) | `:676` | commit combos → `CommPort`, `BaudRate`, then `SerialPortOpen(true)` |
| **Auto-Detect** | `:683` | clears grid, runs the detection BackgroundWorker |
| **Use Selected** | `:693` | commit the highlighted grid row (see §6) |
| Cancel | `:736` | sets `CancelFlag = true`, closes (aborts a running detect) |
| Device Manager | `:742` | `Process.Start("devmgmt.msc")` |
| URL Setup | `:772` | opens `FrmURLDataSetup`; visible **only** when the port is `URL` |

There is no `RunWorkerCompleted` result handling beyond restoring the cursor
(`FrmCommPS.cs:959`). The grid is populated from the worker thread via `Control.Invoke`
(`FrmCommPS.cs:922`), one row per port as it finishes.

---

## 3. Port enumeration — `Common_PS_RF.GetComPorts` (`Common_PS_RF.cs:511`)

```csharp
string[] portNames = SerialPort.GetPortNames();
foreach (...)
{
    string input = Regex.Replace(portNames[i], "[^0-9]", "");   // "COM3" -> "3"
    if (!arrayList.Contains(input)) arrayList.Add(input);
}
arrayList.Sort(new ArrayListComparer());
c.Items.Add("None");
foreach (...) c.Items.Add(digits);
if (_RadioType == BCD536HP || _RadioType == SDS200 || _RadioType == SDS200E)
    c.Items.Add("URL");
if (c.SelectedIndex == -1) c.SelectedIndex = 0;
```

Critical details:

- **Ports are stored as bare digits, not `COM` names.** `COM3` becomes `3`. The `COM` prefix
  is re-attached only at open time (`FrmMain.cs:30007`: `PortName = "COM" + Main.CommPort`).
  This is why the config file holds `[COMM PORT]=3` and not `COM3`.
- **The digits are de-duplicated as strings**, so `COM3` and `COM03` collapse to one entry.
- **Sorted numerically** (`ArrayListComparer`), so `COM10` sorts after `COM9` — not the
  lexicographic `COM1, COM10, COM2` bug you get from naively sorting names.
- **`"None"` is always first**, which is why a fresh profile silently ends up with `None` as
  `SelectedIndex == 0`.
- **`"URL"` is appended only for `BCD536HP`, `SDS200`, `SDS200E`** — the three models ProScan
  considers network-capable. Every other model is serial-only and gets no network option.

> **Quirk — `NONE` vs `None`, and it's deliberate.** `"NONE"` is set explicitly in two places,
> not a stray default: the factory-reset block (`General.cs:6572`, alongside `CommType = 0` and
> `BaudRate = 0`) and — importantly — `RadioSpecific.RS_InitRadioType()` (`RadioSpecific.cs:166`).
> The combo box item is `"None"`, so on load `ComboBox1.Text = Main.CommPort` matches nothing,
> `SelectedIndex` stays `-1`, and the `== -1` fallback snaps it to index 0 (`"None"`). It works,
> but by accident. `SerialPortOpen` tests `CommPort.ToUpper() == "NONE"` to catch both forms
> (`FrmMain.cs:29993`).

> **Quirk — changing scanner type can silently wipe your port.** `RS_InitRadioType()` runs on
> every scanner-type change and contains (`RadioSpecific.cs:164-169`):
>
> ```csharp
> if (Main.CommPort == "URL")
> {
>     Main.CommPort = "NONE";
>     Main.fMain.SerialPortClose();
>     DisplaysOff();
> }
> ```
>
> So switching from a URL-capable model (`BCD536HP`/`SDS200`/`SDS200E`) to any other model
> **resets the port to `NONE` and closes it** — the right behaviour, since the new model has no
> network transport, but it means your port setting disappears without a prompt. A numeric COM
> port is **not** affected; only `URL` is reset.

---

## 4. Baud rates — `RadioSpecific.RS_AddBaudData` (`RadioSpecific.cs:2594`)

A hard `switch` on `Main._RadioType` returning a `string[]`. The combo is filled directly with
it (`FrmCommPS.cs:622`), and the head of each array is the recommended/default rate.

| Models | Offered baud rates |
|---|---|
| `BC250D`, `BC780XLT`, `BC785D` | 19200, 9600, 4800, 2400 |
| `BC296D`, `BC796D` | 57600, 38400, 19200, 9600, 4800, 2400 |
| `BCT15`, `BCT15X`, `BR330T`, `BC346XT`, `BC346XTC`, `BCD325P2`, `BCD396T`, `BCD396XT`, `BCD436HP`, `BCD536HP`, `BCD996T`, `BCD996XT`, `BCD996P2`, `SDS100`, `SDS150`, `SDS200`, `SDS100E`, `SDS200E`, `UBCD3600XLT`, `USDS100`, `UBCD436PT`, `UBCD536PT` | 115200, 57600, 38400, 19200, 9600, 4800 |
| `BCD160DN`, `BCD260DN`, `BC125AT`, `UBCD160DN`, `UBCD260DN`, `UBC125XLT`, `UBC126AT` | **115200 only** |
| anything else | `[""]` — one empty entry |

Two things to notice:

- The **DN/125AT family is locked to 115200**. There is no dropdown to choose otherwise; if
  one of those is stuck at a lower rate, ProScan cannot talk to it at all.
- The `default` branch returns a single **empty string**, and `FrmCommPS` writes
  `ComboBox2.Text = Main.BaudRate.ToString()` (`:623`). A `BaudRate` of `0` therefore renders
  as `"0"`, which matches no entry, and the `SelectedIndex == -1` fallback (`:624`) selects
  index 0 — i.e. the empty string. `Main.BaudRate` stays `0`, and `SerialPortOpen` then treats
  it as *not a valid baud* (§7). This is the mechanism behind `[BAUD RATE]=0` in a fresh profile.

---

## 5. Auto-Detect — `AutoDetectPortsBackgroundWorker_DoWork` (`FrmCommPS.cs:793`)

This is the useful part. It is a brute-force serial probe.

### The baud list it tries is NOT the model's list

```csharp
int[] array = new int[7] { 115200, 57600, 38400, 19200, 9600, 4800, 2400 };
```

A **fixed** array, hardcoded, independent of `RS_AddBaudData`. So auto-detect can report a
baud rate the Scanner Type menu does not offer for the selected model — e.g. it will happily
find an SDS200 at 2400, which §4's dropdown cannot express. Treat a detected rate as
information, not as something the UI can represent.

### Algorithm

```
ports = SerialPort.GetPortNames() -> digits only -> dedupe -> numeric sort
for each port N:
    port.PortName = "COM" + N
    status = "", model = "", foundBaud = ""
    for j, baud in [115200, 57600, 38400, 19200, 9600, 4800, 2400]:
        if CancelFlag: return
        port.BaudRate = baud
        port.ReadTimeout = 1; port.WriteTimeout = 500
        port.Encoding = Encoding.GetEncoding(28591)   # ISO-8859-1
        port.RtsEnable = true; port.DtrEnable = true
        try:
            port.Open()
            if j == 0: status = "Available"
            port.Write("\rMDL\r"); Thread.Sleep(50)
            resp = port.ReadExisting()
            if resp contains "MDL,":
                model = resp.Substring(idx + 4)
                foundBaud = baud; port.Close(); break
            port.Write("\rSI\r"); Thread.Sleep(50)
            resp = port.ReadExisting()
            if resp contains "SI ":
                tail = resp.Substring(idx + 3)
                model = tail.Substring(0, tail.IndexOf(","))
                foundBaud = baud; port.Close(); break
        except:
            if j == 0 and ex contains "Access" and "denied":
                status = (CommPort == N and our port is open) ? "In Use By This Instance"
                                                              : "In Use By Another Program"
            port.Close(); break
    if status == "": status = "Check Driver"
    add grid row: (N, status, model, foundBaud)
```

### The wire protocol

Two Uniden ASCII commands, each terminated `\r`, with a **50 ms** settle before reading:

| Sent | Success marker | Extraction |
|---|---|---|
| `\rMDL\r` | `"MDL,"` | everything after `MDL,` |
| `\rSI\r` | `"SI "` | after `SI `, up to the first `,` |

`MDL` is the modern model-ID command and is tried first. `SI` is the older status-info command,
kept as a fallback for legacy models. A scanner is only accepted if the response *contains* the
marker anywhere — the parser doesn't anchor to the start of the buffer.

### Status column values

| Status | Meaning |
|---|---|
| `Available` | Port opened successfully at 115200 (only ever set on `j == 0`) |
| `In Use By This Instance` | Access denied, and it's ProScan's own open port |
| `In Use By Another Program` | Access denied, someone else holds it |
| `Check Driver` | Port never opened at all — no serial exception, but no success either |

`Check Driver` is the catch-all for a phantom port (Bluetooth outgoing stubs, disconnected USB
adapters, stale registry entries). It's the one that tells you the port exists in Windows but
nothing is behind it. The Device Manager button (`FrmCommPS.cs:742`) is there for exactly this.

> **Note on the port-reuse pattern.** The `SerialPort` object is created once outside the port
> loop and reused, with `val.Close()` in the `for` increment clause. A port that throws on open
> therefore `break`s the baud loop and moves to the next port rather than retrying at a lower
> rate — if a device only answers at 9600 but errors on the 115200 open, detection gives up on it.

---

## 6. "Use Selected" — `Button4_Click` (`FrmCommPS.cs:693`)

Enabled only when the highlighted row's Status **contains `"Available"`**
(`DataGridView15_SelectionChanged`, `FrmCommPS.cs:781`) — so a `Check Driver` or in-use row is
unclickable.

```csharp
Main.CommPort = row.Cells[0];                       // digits
ComboBox1.Text = Main.CommPort;
Main.BaudRate = int(row.Cells[3]);                  // detected baud
ComboBox2.Text = Main.BaudRate.ToString();
if (row.Cells[2] != "" && row.Cells[2] != _RadioType.ToString())
{
    if (!ClosingDB("8")) return;                    // confirm — model change
    Main._RadioType = Enum.Parse(row.Cells[2]);
    RadioSpecific.RS_InitRadioType();
}
Main.fMain.SerialPortOpen(ShowMessage: true);
```

So **one click can change the scanner type as well as the port** — the detected model string
from `MDL`/`SI` is parsed straight back into the enum. If the detected model isn't a
`RadioType` member the `Enum.Parse` throws and is swallowed (`:726`), leaving the old type in
place but the new port committed.

This is the shortcut worth knowing: **Auto-Detect → pick the `Available` row → Use Selected**
sets port, baud, *and* model in one action, and is more reliable than picking the model by hand
from a 34-entry alphabetical list.

---

## 7. Opening the port — `FrmMain.SerialPortOpen` (`FrmMain.cs:29976`)

```csharp
if (Main.ProgramClosing || Main.SerialPort1 == null) return;
SerialPortClose();
if (CommPort is null or "")            { "Invalid Serial Port"; return; }
if (CommPort.ToUpper() == "NONE")      { return; }            // silent no-op
if (CommPort == "URL" || BaudRate != 0)
{
    ...
    if (CommPort != "URL") { PortName = "COM" + CommPort; BaudRate = Main.BaudRate; }
    else                   { PortName = CommPort; }           // literally the string "URL"
    ReadTimeout = 1; WriteTimeout = 500;
    Encoding = Encoding.GetEncoding(28591);                    // ISO-8859-1
    RtsEnable = true; DtrEnable = true; DiscardNull = true;
    Open();
    StatusBar0Write("COMM PORT Open, Port " + CommPort + ", Baud Rate " + BaudRate);
    TimerPollStart();
}
else                                   { "Invalid Baud Rate"; }
```

Key semantics:

- **`NONE` is a silent no-op.** No error dialog — the app just doesn't open anything. This is
  the "why is nothing happening" state.
- **The open condition is `CommPort == "URL" || BaudRate != 0`.** A real port with
  `BaudRate == 0` is rejected with *"Invalid Baud Rate"* — which is exactly the state a fresh
  profile lands in (§4).
- **`COM` is prefixed here**, never stored (`"COM" + Main.CommPort`).
- **URL mode bypasses the baud rate entirely** (`PortName = "URL"`), which matches the dialog
  hiding the baud combo when `URL` is selected (`FrmCommPS.cs:756`).
- Open failure messages differ by transport: `"Unable To Open Comm. Port\r\rCheck If Other
  Software Is Using Port N"` vs `"Unable To Connect To UDP Port <URLDataList[0]>:<URLDataUDPScannerPort>"`.
- Each of these message paths is latched by a static flag, so the dialog appears **once** per
  retry cycle, not on every failure.
- On success, `TimerPollStart()` starts the polling loop that actually drives the scanner — the
  port being open is not the same as the scanner responding (`Main.RadioResponding` tracks that
  separately and drives the status-bar LEDs).

---

## 8. `MySerialPort` — one object, two transports (`MySerialPort.cs`)

Not a subclass of `SerialPort` — a **wrapper** holding either a `SerialPort` or a UDP `Socket`,
switched by `_Mode`:

| `_Mode` | Meaning | Set when |
|---|---|---|
| `0` | initial, unopened | constructor (`:138`) |
| `1` | real serial port | `Open()` when `PortName != "URL"` (`:150`) |
| `2` | UDP socket | `Open()` when `PortName == "URL"` (`:154`) |

The mode is decided **by string comparison against `"URL"`** (`MySerialPort.cs:148`) — the same
magic value, tested independently in three places (`GetComPorts`, `SerialPortOpen`,
`MySerialPort.Open`). There is no enum and no constant; `"URL"` is a bare literal each time.

UDP branch (`:170`):

```csharp
Main.URLDataBytesTX = 0; Main.URLDataBytesRX = 0;
Main.URLDataStartTime = DateTime.Now; Main.URLDataUDPConnected = false;
_UDPSocket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
_UDPSocket.Connect(Main.URLDataList[0], int.Parse(Main.URLDataUDPScannerPort));
Main.URLDataUDPComputerPort = ((IPEndPoint)_UDPSocket.LocalEndPoint).Port.ToString();
_IsOpen = true;
_UDPSocket.BeginReceive(..., OnDataReceived, ...);
```

`Write` (`:185`) sends datagrams chunked at **1400 bytes** — the scanner's UDP transport takes
an MTU-safe payload, not a byte stream. Destinations come from `FrmURLDataSetup` (the URL Setup
button), stored in `Main.URLDataList[0]` / `Main.URLDataUDPScannerPort`.

---

## 9. LAN discovery of networked scanners — `FrmAutoDetectIPScanners.cs`

Separate from the serial probe. This is the Uniden UDP discovery broadcast, and it's reusable:

```
For each up NetworkInterface with an IPv4 unicast address:
    prefix = first three octets of the address          # e.g. "192.168.2"
    (fallback if none found: "255.255.255")
If prefix can be bound:
    socket = UdpClient(50536); ReceiveTimeout = 2000 ms
    payload = ASCII("SUS,UNIDEN,SCANNER\r")
    send payload -> <prefix>.255 : 50536                # subnet broadcast first
    read replies; accept if starts with "SUS," and has exactly 4 commas
Then unconditionally:
    send payload -> 255.255.255.255 : 50536             # global broadcast fallback
    read replies, same acceptance test
```

Provenance: `FrmAutoDetectIPScanners.cs:339` (interface enumeration), `:366` (payload),
`:372` (bind 50536), `:386` / `:407` (the two broadcasts), `:393` / `:414` (the acceptance test).

Notes:
- **UDP 50536 both ways.** Local bind and destination port are the same number.
- **The response format is `SUS,` + exactly 4 commas = 5 fields.** Anything else is discarded.
  This is a positional CSV, not keyed.
- **Two-phase**: subnet broadcast, then global broadcast. The per-interface prefix is derived by
  string-splitting the address and taking three octets, so it is always a **/24** — a /16 or
  /23 LAN will only scan one /24 via the first phase and depend entirely on the global
  broadcast for the rest.
- If binding 50536 fails (something else holds it), both phases are skipped silently.

> **For your metadata tool:** this discovery protocol is the cheapest way to enumerate Uniden
> network scanners on a LAN without port-scanning. It is plain UDP, no auth, no handshake —
> send `SUS,UNIDEN,SCANNER\r` to port 50536 and read the CSV reply.

---

## 10. Persistence — `ProScan.cfg`

UTF-16LE INI, CRLF, written by `General.cs:24504-24516` and parsed at `General.cs:8714-8768`.
The four relevant keys:

```
[SCANNER TYPE]=BCD996XT
[COMM TYPE]=0
[COMM PORT]=NONE
[BAUD RATE]=0
```

Read logic (`General.cs:8714`):

```csharp
Main._RadioType = (Main.RadioType)int.Parse(
    Enum.Parse(typeof(Main.RadioType), text2.Replace("NONE", "None")));
```

- The `NONE` → `None` rewrite handles the unset-scanner **port** sentinel leaking into the
  **scanner type** key. Note `RadioType` has no `None` member, so `Enum.Parse("None")` throws —
  the outer catch swallows it and the previous `_RadioType` stands. Sloppy but harmless.
- `Enum.Parse` on a numeric string also works, so the key tolerates both model names and raw
  enum ordinals.
- `[COMM PORT]` is stored **as-is** (`:8750`) with an empty-string guard, preserving the
  uppercase `NONE` vs `None` split from §3.

### `[COMM TYPE]` is dead code

`Main.CommType` (`Main.cs:1283`) is written to the config and read back (`General.cs:8732`) and
is set to `0` once (`General.cs:6568`) — and **never read for any decision**. A search across
the whole decompile finds no comparison against it. The actual transport choice is made purely
by the `CommPort == "URL"` string test (§7, §8). It's 24.13-era vestigial state, probably a
remnant of a serial/TCP/URL mode selector that was replaced by the `"URL"` pseudo-port.

---

## 11. Quirks worth knowing

1. **Ports are digits.** `"3"`, not `"COM3"`. `COM` is added only at open. Any tool
   interoperating with `ProScan.cfg` must emit bare digits.
2. **`NONE` (config) ≠ `None` (dropdown).** `"NONE"` is set deliberately by the factory reset
   (`General.cs:6572`) and by `RS_InitRadioType` (`RadioSpecific.cs:166`); the UI works only via
   case-insensitive compares and a `SelectedIndex == -1` fallback.
3. **Changing scanner type silently resets `URL` → `NONE`** (`RadioSpecific.cs:164-169`). Numeric
   COM ports survive a model change; `URL` does not.
4. **`URL` is a bare string literal in three independent places**, not a constant. Introduced
   only for `BCD536HP` / `SDS200` / `SDS200E`.
5. **Auto-detect uses its own fixed baud list**, ignoring `RS_AddBaudData` — it can report rates
   the UI cannot express.
6. **DN/125AT models are 115200-only** with no override in the UI.
7. **`BaudRate == 0` + a real port = "Invalid Baud Rate"**, and `NONE` = silent no-op. Both are
   the default state of a fresh profile.
8. **`GetPortNames()` digits-only filtering is lossy.** Virtual ports whose names contain no
   digits collapse to `""` and become an empty, unopenable entry.
9. **The scanner-type list is alphabetical, not grouped** by anything meaningful, and
   reselecting the current model is a silent no-op with no feedback.
10. **`Check Driver` is the phantom-port signal** — the port exists to Windows, nothing answered.
11. **LAN discovery is /24-only per interface**, with a global broadcast fallback.

---

## Verify it yourself

Everything above is readable in the decompile:

```bash
cd ~/workspace/proscan/decompiled
sed -n '511,554p' Common_PS_RF.cs     # GetComPorts
sed -n '2594,2640p' RadioSpecific.cs  # RS_AddBaudData
sed -n '793,957p' FrmCommPS.cs        # the auto-detect probe
sed -n '29976,30060p' FrmMain.cs      # SerialPortOpen
sed -n '143,183p' MySerialPort.cs     # serial vs UDP mode switch
grep -n "SUS,UNIDEN,SCANNER" FrmAutoDetectIPScanners.cs
```

Field-check against a real install: the `[COMM PORT]`/`[BAUD RATE]` values in `ProScan.cfg`
after changing the port in the UI, and the Port/Status/Scanner/Baud row for your scanner in the
auto-detect grid. Anything that disagrees with this document — the installed behaviour wins.
