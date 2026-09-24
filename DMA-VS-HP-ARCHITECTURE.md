# ProScan per-model command architecture — DMA vs HP/SDS

Reverse-engineered from `ProScan.exe` (VB.NET, .NET 4.8, ILSpy 7.2.1). Every claim carries
`file:line` in `decompiled/`. Facts marked **PROVEN** are read directly from code; **INFERENCE**
marks a conclusion drawn from usage rather than a comment/spec.

Two orthogonal per-model abstractions exist:

- `Main.ScannerClass : int` (`Main.cs:1115`) — 5 behaviour classes, assigned **once** in
  `RadioSpecific.Common()`. **This is the real behaviour-grouping key.**
- `Main.ScannerType : string` (`Main.cs:1261`) — the raw `MDL` response string, used only for
  identity checks and one substring heuristic.

---

## 1. MODEL DETECTION

### 1.1 The MDL string is the enum name, verbatim — no translation table

The `MDL` response is stored raw:

```csharp
Main.ScannerType = General.GetData(InData, "MDL,", 5);      // FrmMain.cs:21840
```

`General.GetData(str1, key, start)` (`General.cs:28025`) locates `"\r" + key` with
`InStr`, finds the following `"\r"`, and returns `Strings.Mid(str1, num + start, num2 - num - start)`.
With `key = "MDL,"`, `start = 5` (the 5 chars `\rMDL,` are skipped) the result is **exactly** the
model token between `MDL,` and the next CR. Same call at `FrmMain.cs:22554`, `:22754`, `:23208`,
`:23242`.

That string is then compared **directly against the enum member's name**:

```csharp
// FrmMain.cs:37782
if (!Main.BypassScannerCheck && Operators.CompareString(Main.ScannerType, Main._RadioType.ToString(), true) != 0)
{
    ...
    Common_PS_RF.Message(owner, "The Scanner Type Does Not Match The ProScan Scanner Type\r\rSwitch ProScan Scanner Type Before Continuing");
```

The port dialog does the same, then `Enum.Parse` on the detected text:

```csharp
// FrmCommPS.cs:715
if (Operators.CompareString(Main._RadioType.ToString(), Conversions.ToString(val.get_Cells().get_Item(2).get_Value()).Trim(), true) != 0 && ...)
...
// FrmCommPS.cs:723
Main._RadioType = (Main.RadioType)Conversions.ToInteger(Enum.Parse(typeof(Main.RadioType), Conversions.ToString(val.get_Cells().get_Item(2).get_Value()).Trim()));
```

**Conclusion (PROVEN): the `MDL` response string matches the `Main.RadioType` member name exactly,
compared case-insensitively. There is no model-name → enum translation table.** Every `RadioType`
value in the program is produced by `Enum.Parse(typeof(Main.RadioType), <string>)` on either the
`MDL` token, the config file, or the combo box text — `FrmCommPS.cs:723`, `FrmMain.cs:34683`,
`FrmMain.cs:36280`, `General.cs:8714`, `FrmURLDataSetup.cs:577`, `DynamicDatabase_D.cs:19561`.
A search for a name→enum lookup table returned nothing; `grep -rn 'Enum.Parse(typeof(Main.RadioType)' .`
returns exactly the 10 sites above (6 in the main app, 4 in `Profile_Editor_exe/`).

### 1.2 The only two places a partial model string *is* string-matched

Both are property probes, not enum mapping:

```csharp
// FrmMain.cs:21842 — pads the STS payload for 396/330/346-class screens
if (Main.ScannerType.Contains("396") || Main.ScannerType.Contains("330") || Main.ScannerType.Contains("346"))
{
    text += ",,,,,";
}
```

```csharp
// FrmMain.cs:20845 — BC346XT/XTC/BCD396XT/BCD325P2 + "3" in MDL response
if ((Main._RadioType == Main.RadioType.BC346XT || ... == Main.RadioType.BCD325P2) && flag && TxrxSerialPortDLULModes("MDL\r").Contains("3"))
```

### 1.3 The real translation table is for *database* conversion, not enum mapping

`DynamicDatabase_B.GetFromClass` / `GetToClass` map model names to three DMA sub-classes by
plain string comparison — this is the closest thing to a mapping table in the app, and it exists
only for DB import/export shaping:

```csharp
// DynamicDatabase_B.cs:71720
private string GetFromClass(string Type)
{
    if (Operators.CompareString(Type, "BCD996T", true) == 0 || Operators.CompareString(Type, "BCT15", true) == 0)
        return "FROM_996";
    if (Operators.CompareString(Type, "BCD396T", true) == 0 || Operators.CompareString(Type, "BR330T", true) == 0)
        return "FROM_396";
    if (Operators.CompareString(Type, "BCD996XT", true) == 0 || Operators.CompareString(Type, "BCT15X", true) == 0
        || Operators.CompareString(Type, "BCD396XT", true) == 0 || Operators.CompareString(Type, "BC346XT", true) == 0
        || Operators.CompareString(Type, "BC346XTC", true) == 0 || Operators.CompareString(Type, "BCD325P2", true) == 0
        || Operators.CompareString(Type, "BCD996P2", true) == 0)
        return "FROM_XT";
    return string.Empty;
}
```

`GetToClass` (`DynamicDatabase_B.cs:71737`) is the mirror image (`TO_996` / `TO_396` / `TO_XT`).
So the DMA family itself has an internal 3-way split: **996-class, 396-class, XT-class**
(the XT bucket explicitly includes `BCD996P2`, `BCD325P2`, `BCD996XT`, `BCD396XT`, `BC346XT/XTC`, `BCT15X`).

### 1.4 `Main.ScannerClass` — assignment and meaning

`Main.ScannerClass` is declared `public static int ScannerClass;` (`Main.cs:1115`). It is assigned
in exactly **five** places in the entire decompile, all inside `RadioSpecific.Common()`:

| Line | Members | Class |
|---|---|---|
| `RadioSpecific.cs:4558-4563` | `BC250D`, `BC296D`, `BC780XLT`, `BC785D`, `BC796D` | **1** |
| `RadioSpecific.cs:4621-4632` | `BCT15`, `BCT15X`, `BR330T`, `BC346XT`, `BC346XTC`, `BCD325P2`, `BCD396T`, `BCD396XT`, `BCD996T`, `BCD996XT`, `BCD996P2` | **2** |
| `RadioSpecific.cs:4688-4690` | `BCD536HP`, `UBCD536PT` | **3** |
| `RadioSpecific.cs:4722-4731` | `BCD436HP`, `SDS100`, `SDS150`, `SDS200`, `SDS100E`, `SDS200E`, `UBCD3600XLT`, `USDS100`, `UBCD436PT` | **4** |
| `RadioSpecific.cs:4764-4772` | `BCD160DN`, `BCD260DN`, `BC125AT`, `UBCD160DN`, `UBCD260DN`, `UBC125XLT`, `UBC126AT` | **5** |

```csharp
// RadioSpecific.cs:4621
case Main.RadioType.BCT15:
case Main.RadioType.BCT15X:
case Main.RadioType.BR330T:
case Main.RadioType.BC346XT:
case Main.RadioType.BC346XTC:
case Main.RadioType.BCD325P2:
case Main.RadioType.BCD396T:
case Main.RadioType.BCD396XT:
case Main.RadioType.BCD996T:
case Main.RadioType.BCD996XT:
case Main.RadioType.BCD996P2:
    Main.ScannerClass = 2;
    Main.VirtualScanner1.FrontPanel_DMA1 = new FrontPanel_DMA();
    ...
```

Semantics, proven from usage:

| Class | Meaning (PROVEN by what `Common()` builds + consumers) | Front panel object |
|---|---|---|
| 1 | Legacy Bearcat "A" family — keypad/LCD menu protocol (`Control_A`, `Keypad_A`, `DynDB_A`) | `Display1` + `Keypad_A1` + `Control_A1` (`RadioSpecific.cs:4565-4566`) |
| 2 | **DMA family** (BCD996P2 / BCD325P2 / BCD996XT / BCD396XT …) | `FrontPanel_DMA` (`RadioSpecific.cs:4634`), `DynDB_B` (`:4653`) |
| 3 | `BCD536HP` / `UBCD536PT` only | `FrontPanel_HP` (`RadioSpecific.cs:4692`) |
| 4 | HP/SDS family (BCD436HP, SDS100/150/200, SDS100E/200E, UBCD3600XLT, USDS100, UBCD436PT) | `FrontPanel_SDS` (`RadioSpecific.cs:4733`) |
| 5 | DN/125AT family (BCD160DN/260DN, BC125AT, …) — **also uses `FrontPanel_DMA`** (`RadioSpecific.cs:4773`) but a different parser and no `GSI` | `FrontPanel_DMA` |

Class-dependent database panel, a clean confirmation of the grouping:

```csharp
// FrmMain.cs:26992
if (Main.ScannerClass == 1)            { if (Main.DynDB_A != null) return Main.DynDB_A.RecordDatabaseLookup(); }
else if (Main.ScannerClass == 2)       { if (Main.DynDB_B != null) return Main.DynDB_B.RecordDatabaseLookup(); }
else if (Main.ScannerClass == 3 || Main.ScannerClass == 4) { if (Main.DynDB_C != null) return Main.DynDB_C.RecordDatabaseLookup(); }
else if (Main.ScannerClass == 5 && Main.DynDB_D != null)   { return Main.DynDB_D.RecordDatabaseLookup(); }
```

`Display.cs:523` additionally tests `Main.ScannerClass == 0` — the uninitialised state before
`RS_InitRadioType()` runs.

**Note for a re-implementation:** class 2 and class 5 share the `FrontPanel_DMA` widget but not the
parser; class 3 and class 4 share the parser (`ProcessRXData_Uniden_536HP` /
`ProcessRXData_Uniden_436HP_SDS100` both fall through to `ProcessRXData_Uniden_Common_HP`) but not
the front panel. Grouping by "class" alone is therefore insufficient — the parser dispatch is a
`switch (Main._RadioType)` on the model, not on the class (`FrmMain.cs:21117-21199`).

---

## 2. POLLING COMMAND PER FAMILY

The poll command string is built by `RadioSpecific.RS_NormPolls()` (`RadioSpecific.cs:1000`) into
`Main.txPoll`, and sent by the poll timer:

```csharp
// FrmMain.cs:20930 TimerPollSerialEvent
if (!Main.ProgramClosing && (Main.OperationMode == 0 || Main.OperationMode == 2) && Main.SerialPort1 != null && Main.SerialPort1.IsOpen)
{
    if (Operators.CompareString(Main.KeyPressedFlag, string.Empty, true) == 0)
        RadioSpecific.RS_NormPolls();
    else
        Main.KeyPressedFlag = string.Empty;
    try
    {
        rxReturnPoll = "START\r" + SerialPortSendReceive.SerialPortSendReceivePoll(Main.txPoll.ToString());
        ...
```

`SerialPortSendReceivePoll` writes the whole `Main.txPoll` string in a single `Write(SendData)`
(`SerialPortSendReceive.cs:48`) — all classes use one write; only the read-termination differs.

### The switch — `RadioSpecific.cs:1013-1060`

**Class 2 DMA (BCD996P2, BCD325P2, BCD996XT, BCD396XT, BCT15/X, BR330T, BC346XT/XTC, BCD396T, BCD996T)
— and class 5:**

```csharp
// RadioSpecific.cs:1024
case Main.RadioType.BCT15:
case Main.RadioType.BCT15X:
case Main.RadioType.BR330T:
case Main.RadioType.BC346XT:
case Main.RadioType.BC346XTC:
case Main.RadioType.BCD325P2:
case Main.RadioType.BCD396T:
case Main.RadioType.BCD396XT:
case Main.RadioType.BCD996T:
case Main.RadioType.BCD996XT:
case Main.RadioType.BCD996P2:
case Main.RadioType.BCD160DN:
case Main.RadioType.BCD260DN:
case Main.RadioType.BC125AT:
case Main.RadioType.UBCD160DN:
case Main.RadioType.UBCD260DN:
case Main.RadioType.UBC125XLT:
case Main.RadioType.UBC126AT:
    Main.txPoll.Append("MDL\rSTS\rGLG\rVOL\rSQL\rPWR\r");          // RadioSpecific.cs:1042
    break;
```

**Class 3/4 HP/SDS (BCD436HP, BCD536HP, SDS100, SDS150, SDS200, SDS100E, SDS200E, UBCD3600XLT,
USDS100, UBCD436PT, UBCD536PT):**

```csharp
// RadioSpecific.cs:1044
case Main.RadioType.BCD436HP:
case Main.RadioType.BCD536HP:
... SDS100, SDS150, SDS200, SDS100E, SDS200E, UBCD3600XLT, USDS100, UBCD436PT, UBCD536PT ...
    Main.txPoll.Append("MDL\rSTS\rGLG\rVOL\rSQL\rPWR\rGSI,1\r");    // RadioSpecific.cs:1055
    if (Main.ScannerData1.InfoMode.Contains("Waterfall"))
    {
        Main.txPoll.Append("GWF,1\r");                             // RadioSpecific.cs:1058
    }
    break;
```

**Class 1 legacy (BC250D/BC296D/BC780XLT/BC785D/BC796D):**

```csharp
// RadioSpecific.cs:1013
case Main.RadioType.BC250D:
case Main.RadioType.BC296D:
    Main.txPoll.Append("IDN\rRIF\rQUF\rDMF\rMD\rSQ\rSG\rTB\rAW\rCDN\rLCD\r");   // :1015
    break;
case Main.RadioType.BC780XLT:
    Main.txPoll.Append("IDN\rRIF\rQUF\rMD\rSQ\rSG\rTB\rCDN\rLCD\r");            // :1018
    break;
case Main.RadioType.BC785D:
case Main.RadioType.BC796D:
    Main.txPoll.Append("IDN\rRIF\rQUF\rDMF\rMD\rSQ\rSG\rAR\rTB\rAW\rCDN\rLCD\r"); // :1022
    break;
```

### Literal audit (each candidate from the brief)

| Candidate | Found? | Where |
|---|---|---|
| `"MDL\r"` | **PROVEN** | poll prefix for classes 2/3/4/5 (`RadioSpecific.cs:1042`, `:1055`); DLUL probe `"MDL\r"` many sites (`DynamicDatabase_B.cs:31701`, `FrmMain.cs:20845`) |
| `"STS\r"` | **PROVEN** | poll element for classes 2/3/4/5 (`RadioSpecific.cs:1042`, `:1055`); also `FrmMain.cs:20834`, `Shortcuts.cs:708` |
| `"GLG\r"` | **PROVEN** | poll element for classes 2/3/4/5 (`RadioSpecific.cs:1042`, `:1055`) |
| `"VOL\r"`, `"SQL\r"`, `"PWR\r"` | **PROVEN** | same literals |
| `"GSI,1\r"` | **PROVEN** | HP/SDS only (`RadioSpecific.cs:1055`) |
| `"GWF,1\r"` | **PROVEN** | HP/SDS only, conditional on `InfoMode` containing `"Waterfall"` (`RadioSpecific.cs:1056-1058`) |
| `"LCD\r"` | **PROVEN, but class 1 only** | `RadioSpecific.cs:1015`, `:1018`, `:1022`; class-1 DLUL lists (`DynamicRadiowide_A.cs:3107`). Never in a DMA poll. |
| `"SI\r"` | **PROVEN, not a poll** | only the auto-detect fallback probe `FrmCommPS.cs:873` (`\rSI\r`) and class-1 DLUL command lists |
| `"GOM\r"` | **NOT FOUND** | searched `"GOM`, `GOM\r`, `GOM,` across all `*.cs` — zero hits |
| `"MSI\r"` | **NOT FOUND** | searched `"MSI`, `MSI\r`, `MSI,` — only hit is the unrelated browser UA test `"MSIE"` (`WebServer.cs:7474`) |

**PROVEN bottom line:** DMA and HP/SDS send *the identical six commands*
(`MDL`, `STS`, `GLG`, `VOL`, `SQL`, `PWR`); the only wire difference is the appended `GSI,1`
(and optional `GWF,1`) on HP/SDS. The divergence is entirely in the **response parser** and in the
read-termination condition.

### Read termination also splits by family

`SerialPortSendReceivePoll`'s read loop switches on `Main._RadioType` (`SerialPortSendReceive.cs:78`).
DMA/125/160-260 accepts a response as soon as it contains `STS,` and `GLG,` and ends with CR
(`:126`). HP/SDS requires the `GSI` marker as well (or that XML was never seen):

```csharp
// SerialPortSendReceive.cs:150-160
if (!Main.XMLPresentSerialPort)
{
    if (buffer.Contains("STS,") && buffer.Contains("GLG,") && buffer.EndsWith("\r")) { break; }
}
else if (buffer.Contains("STS,") && buffer.Contains("GLG,") && ((buffer.Contains("GSI,<XML>,") && buffer.Contains("</ScannerInfo>")) || buffer.Contains("GSI,OK")) && buffer.EndsWith("\r"))
{ break; }
```

---

## 3. RESPONSE TOKEN SCHEMA — DMA family (ScannerClass 2)

### 3.1 Framing and delimiter

The DMA poll response is a sequence of CR-terminated frames: `\rMDL,<model>\r…\rSTS,<screen>\r\rGLG,…\r`.
Fields are recovered **by key**, not by position in the whole response:

```csharp
// General.cs:28025
public static string GetData(string str1, string key, int start)
{
    int num = Strings.InStr(str1, "\r" + key, (CompareMethod)1);
    checked
    {
        int num2 = Strings.InStr(num + 1, str1, "\r", (CompareMethod)1);
        if (num2 - num - start > 0) return Strings.Mid(str1, num + start, num2 - num - start);
        return string.Empty;
    }
}
```

Within `GLG` and `STS` the delimiter is `,` and all indices below are **0-based array positions**
from `String.Split(new char[1] { ',' })` — i.e. `tokens[0]` is the first field after the comma that
follows the command name (`"\rGLG,"` is 5 chars, hence the `start` of 5).

### 3.2 `GLG` token map as the DMA parser reads it

All in `ProcessRXData_Uniden_B` (`FrmMain.cs:21800-22542`); array is created at `FrmMain.cs:21863`:

```csharp
// FrmMain.cs:21863
string[] array2 = General.GetData(InData, "GLG,", 5).Split(new char[1] { ',' });
if (array2 != null && array2.Length > 7)
{
    if (Operators.CompareString(array2[7], "1", true) == 0)
    {
        Main.ScannerData1.Activity = true;
    }
    array2[0] = array2[0].Replace("ID:", string.Empty).Trim();
    string[] array3 = General.GetData(InData, "PWR,", 5).Split(new char[1] { ',' });
    if (array2[0].Contains("."))
    {
        Main.ScannerData1.Frequency = General.FormatFreq1(array2[0]).Trim();
    }
    else if (array2[0].Length == 8 && !array2[0].Contains("-") && array3 != null && array3.Length == 2 && Operators.CompareString(array3[1], array2[0], true) == 0)
    {
        Main.ScannerData1.Frequency = General.FormatFreq(array2[0]).Trim();
    }
    else
    {
        if (Operators.CompareString(array2[0], string.Empty, true) == 0 && Operators.CompareString(text2, string.Empty, true) != 0)
        {
            array2[0] = text2;
        }
        Main.ScannerData1.TalkGroup = array2[0].Trim();
    }
    Main.ScannerData1.SystemName  = array2[4].Trim();    // :21888
    Main.ScannerData1.GroupName   = array2[5].Trim();    // :21889
    Main.ScannerData1.ChannelName = array2[6].Trim();    // :21890
```

| GLG index | Field (as used) | Citation |
|---|---|---|
| `[0]` | **Frequency *or* Talkgroup — polymorphic.** Strips `"ID:"`. Contains `"."` → frequency (1 decimal format); 8 digits, no `-`, and equal to `PWR[1]` → frequency (integer format); otherwise → **TalkGroup**. | `FrmMain.cs:21870-21887` |
| `[1]` | Modulation | `FrmMain.cs:21961` |
| `[2]` | **never referenced** in the DMA parser | NOT FOUND |
| `[3]` | Analog tone (PL/DPL), `"0"` = none | `FrmMain.cs:21896-21898` |
| `[4]` | **System** name | `FrmMain.cs:21888` |
| `[5]` | **Department / Group** name | `FrmMain.cs:21889` |
| `[6]` | **Channel** name | `FrmMain.cs:21890` |
| `[7]` | **Activity / squelch flag** — `"1"` → `ScannerData1.Activity = true` | `FrmMain.cs:21864-21869` |
| `[8]` | **never referenced** in the DMA parser | NOT FOUND |
| `[9]` | NumberTag (numeric only) | `FrmMain.cs:21979-21982` |
| `[10]` | Tune number (numeric only) | `FrmMain.cs:21983-21993` |
| `[11]` | **Digital code**: color-code / RAN / RAS / NAC-style 4 hex chars — read only when `GLG[3]` was empty or `"0"` | `FrmMain.cs:21915-21960` |

The parser requires `array2.Length > 7` for the essential fields, `> 11` for `GLG[11]`, and `> 10`
for `GLG[9]/[10]` — evidence that trailing fields are optional/variable-length in the DMA protocol.

### 3.3 Frequency and Talkgroup — actual precedence for DMA

| Source | Wins over | Citation |
|---|---|---|
| `PWR[1]` → `ScannerData1.Frequency`, unconditionally when `PWR` has 2 tokens | everything (it is written **after** the GLG block) | `FrmMain.cs:22068-22073` |
| `GLG[0]` treated as frequency (`.` present, or 8-digit match to `PWR[1]`) | TalkGroup branch | `FrmMain.cs:21872-21879` |
| `GLG[0]` treated as TalkGroup | — | `FrmMain.cs:21886` |
| STS display fallback: `array[7]` sliced 9 chars if char 5 is `.` and char 10 is space; then `array[5]` sliced 9 chars if it contains `"MHz "` | only when Frequency is empty or starts `"0."` | `FrmMain.cs:22074-22096` |
| STS `array[5]` starting `"ID:"` → TalkGroup | only when Activity and TalkGroup empty and no `"--------"` | `FrmMain.cs:22104-22107` |

```csharp
// FrmMain.cs:22068
string[] array4 = General.GetData(InData, "PWR,", 5).Split(new char[1] { ',' });
if (array4 != null && array4.Length == 2)
{
    Main.ScannerData1.Rssi1 = array4[0].Trim();                                 // PWR[0] = RSSI
    Main.ScannerData1.Frequency = General.FormatFreq(array4[1].Trim());         // PWR[1] = frequency
}
```

### 3.4 RSSI and squelch/activity

- **RSSI (primary):** `PWR[0]` → `ScannerData1.Rssi1` (`FrmMain.cs:22071`).
- **RSSI (secondary, screen glyphs):** `STS` text, 1-based `Mid` positions 18-19, byte values
  `166/167` → `"1"`, `168+169` → `"2"`, `170+171` → `"3"`, `172+173` → `"4"`, `174+175` → `"5"`,
  stored in `Rssi2`; if `Rssi1` is empty it is replaced by `Rssi2`:

```csharp
// FrmMain.cs:22153
string rssi = "0";
if (text.Length > 19)
{
    if (Strings.Asc(Strings.Mid(text, 18, 1)) == 166)                { rssi = "1"; }
    else if (Strings.Asc(Strings.Mid(text, 18, 1)) == 167)           { rssi = "2"; }
    else if (Strings.Asc(Strings.Mid(text, 18, 1)) == 168 && Strings.Asc(Strings.Mid(text, 19, 1)) == 169) { rssi = "3"; }
    else if (Strings.Asc(Strings.Mid(text, 18, 1)) == 170 && Strings.Asc(Strings.Mid(text, 19, 1)) == 171) { rssi = "4"; }
    else if (Strings.Asc(Strings.Mid(text, 18, 1)) == 172 && Strings.Asc(Strings.Mid(text, 19, 1)) == 173) { rssi = "5"; }
}
Main.ScannerData1.Rssi2 = rssi;
if (Operators.CompareString(Main.ScannerData1.Rssi1, string.Empty, true) == 0)
{
    Main.ScannerData1.Rssi1 = Main.ScannerData1.Rssi2;
}
```

Note `Strings.Mid(string, start, length)` is **1-based**, so `Mid(text,18,1)` is the 18th character
of the STS payload — this is the only place in the DMA path where a 1-based index appears.

- **Squelch / activity:** `GLG[7] == "1"` sets `Activity = true` (`FrmMain.cs:21866`); it is cleared
  again at the end of the pass if both frequency and talkgroup are empty:

```csharp
// FrmMain.cs:22200
if (Operators.CompareString(Main.ScannerData1.Frequency.Replace("0", string.Empty).Replace(".", string.Empty).Trim(), string.Empty, true) == 0
    && Operators.CompareString(Main.ScannerData1.TalkGroup, string.Empty, true) == 0)
{
    Main.ScannerData1.Activity = false;
}
```

- **Scanner mode / remote mode:** 4 chars right after `STS,` — `General.GetData(InData, "STS", 5, 4)`
  compared against `"1111"` → `GetScannerMode("30")` (`FrmMain.cs:21828-21839`); `"Remote Mode"` +
  `"Keypad Lock"` in the raw buffer → mode `"31"`.

### 3.5 What `DisplayData` gets for DMA

```csharp
// FrmMain.cs:21846
int charPosRev = General.GetCharPosRev(text, ",", 9);
Main.DisplayData = Strings.Mid(text, 1, charPosRev);
```

`GetCharPosRev` (`General.cs:27949`) scans **backwards** from the end and returns the position of
the 9th-from-last comma; `Mid(text, 1, that)` keeps everything **up to and including that comma** —
i.e. the DMA path discards the 8 trailing comma-separated screen fields. There is no
screen-layout-aware parsing here (contrast §4).

The pre-clear for the DMA front panel is a bare 9-field comma string: `Main.DisplayData = ",,,,,,,,,";`
(`FrmMain.cs:21155`).

---

## 4. FAMILY DIVERGENCE — DMA (class 2) vs HP/SDS (classes 3 & 4)

### 4.1 Parser dispatch (per model, not per class)

```csharp
// FrmMain.cs:21144
case Main.RadioType.BCT15:
... BCD325P2, BCD396T, BCD396XT, BCD996T, BCD996XT, BCD996P2 ...
    Main.DisplayData = ",,,,,,,,,";          // 9 fields       (FrmMain.cs:21155)
    ProcessRXData_Uniden_B(rxReturnPoll);                        // FrmMain.cs:21156
    break;
case Main.RadioType.BCD536HP:
case Main.RadioType.UBCD536PT:
    Main.DisplayData = ",,,,,,,,,,,,,,,,,";  // 17 fields      (FrmMain.cs:21164)
    ProcessRXData_Uniden_536HP(rxReturnPoll);                    // FrmMain.cs:21165
    break;
case Main.RadioType.BCD436HP:
... SDS100, SDS150, SDS200, SDS100E, SDS200E, UBCD3600XLT, USDS100, UBCD436PT ...
    Main.DisplayData = ",,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,"; // 44 fields (FrmMain.cs:21176)
    ProcessRXData_Uniden_436HP_SDS100(rxReturnPoll);             // FrmMain.cs:21177
    break;
```

The 9 / 17 / 44 field counts are the physical LCD column counts — the HP/SDS screens are wider.
`ProcessRXData_Uniden_536HP` (`FrmMain.cs:23191`) and `ProcessRXData_Uniden_436HP_SDS100`
(`FrmMain.cs:23224`) are thin wrappers that both end in `ProcessRXData_Uniden_Common_HP(InData)`
(`FrmMain.cs:23220`, `:23275`).

### 4.2 Comparison table

| Aspect | DMA (class 2) | HP/SDS (classes 3/4) | Citation |
|---|---|---|---|
| Parser | `ProcessRXData_Uniden_B` | `…_536HP` → `…_436HP_SDS100` → `ProcessRXData_Uniden_Common_HP` | `FrmMain.cs:21800`, `:23191`, `:23224`, `:23279` |
| Poll suffix | *(none)* | `GSI,1\r` (+ `GWF,1\r` if Waterfall) | `RadioSpecific.cs:1042` vs `:1055` |
| Read-terminate | `STS,`+`GLG,`+ends CR | needs `GSI` marker (unless `!XMLPresentSerialPort`) | `SerialPortSendReceive.cs:126` vs `:158` |
| Screen data (`DisplayData`) | `GetCharPosRev(text,",",9)` + `Mid` — **truncate at 9th-from-last comma, no layout model** | `CommaParser(data)` — **layout-aware**: substitutes `,` with `Chr(224)` inside each display field using per-model field widths | `FrmMain.cs:21846-21847` vs `:23211`, `:23262`, `:24492` |
| Field width source | n/a | `General.GetLengthDisplayFields` switch: `BCD436HP/UBCD3600XLT/UBCD436PT`=24, `SDS100/150/100E/USDS100`=30 (or 24 if token `"0"`), `BCD536HP/UBCD536PT`=36, default 30 | `General.cs:36048-36069` |
| System name | `GLG[4]` | `GLG[4]` (same index!) but only if not already set from XML | `FrmMain.cs:21888` vs `:23416-23418` |
| Group name | `GLG[5]` | from XML `<Department Name=…>` | `FrmMain.cs:21889` vs `:23655-23665` |
| Channel name | `GLG[6]`, else STS `array[5]` `"CH nnn"` suffix | from XML `<ConvFrequency Name=…>` | `FrmMain.cs:21890-21894` |
| Frequency | `PWR[1]` (authoritative), plus `GLG[0]` and STS fallbacks | `PWR[1]` only if mode 0 and frequency empty, via `FormatFreq11` | `FrmMain.cs:22072` vs `:23340-23347` |
| Talkgroup | `GLG[0]` (polymorphic), else STS `array[5]` `"ID:"` | **XML `<ConvFrequency TGID=…>`** (with a `"ID:"` string fallback if XML absent), state-held across polls | `FrmMain.cs:21870-21887`, `:22104` vs `:24100` (ref `TGID`), `:23349-23370` |
| Unit ID | not parsed in the DMA path | XML `U_Id` / `UID:nnnnn` string scan; `UIDLookup()` | `FrmMain.cs:23371-23407` |
| DMR slot | not parsed in the DMA path | XML `RecSlot` → `Slot = "/" + …` | `FrmMain.cs:23640` (ref `Slot`), `:23409-23412` |
| RSSI | `PWR[0]` **unconditionally**; `Rssi2` from STS glyphs; `Rssi1←Rssi2` if empty | `PWR[0]` only if `!= "-999"`; else XML `Rssi` if `!= "-999"` | `FrmMain.cs:22071`, `:22177-22181` vs `:23339-23360` |
| Activity | `GLG[7] == "1"` (absolute index) | `GLG[array.Length - 5] == "1"` (**relative to end**) — plus `DigitalStatus == "Enc"` forces Activity | `FrmMain.cs:21866` vs `:23326`, `:23330-23333` |
| Mode detect | `General.GetData(InData,"STS",5,4) == "1111"` → mode 30 | `DisplayData.StartsWith("1111")` (536HP) / `.StartsWith("1111111111")` or `.Contains(",FTP Mode")` (436/SDS) | `FrmMain.cs:21828` vs `:23213`, `:23264-23271` |
| DLUL framing | `Write(SendData)` raw | `RMT,`/`AUF,` payloads get commas→tabs + an appended decimal checksum of the character codes | `SerialPortSendReceive.cs:221-222` vs `:224-245` |
| DLUL mode wrappers | no `IDF\r`; uses `"4011"`/`"4012"` around operation-mode-7 transfers | same `"4011"`/`"4012"`, **plus `"IDF\r"` and `"4010"` only in the class-1 branch** | class 1: `General.cs:36134`, `:36141`, `:36144`; classes 2–5: `General.cs:36160`, `:36163` |
| Frequency formatting helper | `FormatFreq1` | `FormatFreq3` | `ScannerControl.cs:1612` vs `:1616` |
| Max length, freq/tune entry box | 9 | 10 | `ScannerControl.cs:1105` vs `:1109` |
| Recorder "current channel" | `string.Empty` (class 2) | `string.Empty` (classes 3/4) | `Recorder.cs:1197-1199` |
| Recorder UID source | `UID1` (with `,` → `Chr(44)`) | `UID3` | `FrmMain.cs:26386-26393` |
| Tone filter combo data | `AddToneDataAll_B()` | `AddToneDataAll_C()` | `FrmFindReplace_A_B.cs:3694` vs `:3703` |
| Network transport `"URL"` port | **not offered** | offered only for `BCD536HP`, `SDS200`, `SDS200E` | `Common_PS_RF.cs:132-133` (per existing COMPORT doc) |

Class-scoped fragments worth noting for a re-implementation:

```csharp
// SerialPortSendReceive.cs:217 — DLUL send framing
case 1: Main.SerialPort1.Write(SendData + "\r\r"); break;   // legacy: double CR
case 2:
case 5: Main.SerialPort1.Write(SendData); break;            // DMA + DN/125AT: raw
case 3:
case 4:                                                     // HP/SDS: tab-prefixed + checksum
    if (SendData.StartsWith("RMT,") || SendData.StartsWith("AUF,")) { ... SendData.Replace(",", "\t"); ... sum of Asc() appended ... }
    Main.SerialPort1.Write(SendData);
    break;
```

```csharp
// RadioSpecific.cs:2433 RS_SendOutOfMenu — how to leave a menu differs per class
case 1: TxrxSerialPortDLULModes("MD\r"); ... "KEY11\rMD\r" retry loop ...
case 2:
case 5: Main.fMain.TxrxSerialPortDLULModes("KEY,S,P\r"); break;
case 3:
case 4: if (Main.ScannerMode == 30) Main.fMain.TxrxSerialPortDLULModes("KEY,M,P\r"); break;
```

**INFERENCE (well-supported):** the practical takeaway for adding DMA support is that the port,
baud, framing and even the *polled command set* are identical to what the app already does for
HP/SDS — the DMA work is (a) a comma-token `GLG`/`PWR`/`STS` parser with the fixed indices in §3,
(b) no `GSI` request and a simpler read-termination, (c) a 9-field `DisplayData` pre-clear instead of
the layout-aware `CommaParser` path.

---

## 5. DIGITAL / TONE FIELDS PER FAMILY

### 5.1 Where the values live — `Main.ScannerData` struct (`Main.cs:20`)

| Field | Line | Filled by DMA? | Filled by HP/SDS? |
|---|---|---|---|
| `bool Activity` | `Main.cs:23` | yes — `GLG[7]=="1"` | yes — `GLG[Len-5]=="1"`, or `DigitalStatus=="Enc"` |
| `string Frequency` | `Main.cs:25` | yes — `PWR[1]` primary | yes — `PWR[1]` |
| `string Tone` | `Main.cs:27` | **yes** — analog + digital code + NAC | **never assigned** by `ProcessRXData_Uniden_Common_HP` (verified: no `Tone` assignment in `FrmMain.cs:23279-23578`) |
| `string DMRSlot` | `Main.cs:29` | no | yes — XML `RecSlot` |
| `string TalkGroup` | `Main.cs:31` | yes — `GLG[0]` | yes — XML `TGID` |
| `string SystemName` | `Main.cs:33` | yes — `GLG[4]` | yes — `GLG[4]` or XML `System Name` |
| `string GroupName` | `Main.cs:37` | yes — `GLG[5]` | yes — XML `Department Name` |
| `string ChannelName` | `Main.cs:39` | yes — `GLG[6]` | yes — XML `ConvFrequency Name` |
| `string Modulation` | `Main.cs:45` | yes — `GLG[1]`, or decoded from screen glyph bytes | XML/`PWR` |
| `string Rssi1` | `Main.cs:47` | yes — `PWR[0]` | yes — `PWR[0]` if `!="-999"`, else XML |
| `string Rssi2` | `Main.cs:49` | yes — STS glyph scan (= "1".."5") | not set by the HP path |
| `string UID1/UID2/UID3` | `Main.cs:50/52/54` | UID1 only (activity-keyed) | yes — XML + `UID:` scan |
| `string DisplayedSystemType` | `Main.cs:56` | no | yes — XML `System SystemType` |
| `string DigitalStatus` | `Main.cs:59` | **never assigned in the DMA parser** | yes — XML `<Property P25Status=…>` |
| `string NumberTagTune` | `Main.cs:89` | yes — `GLG[9]` / `GLG[10]` | yes — XML `N_Tag` |

### 5.2 DMA — analog tone (`GLG[3]`)

```csharp
// FrmMain.cs:21896
if (Operators.CompareString(array2[3], "0", true) != 0 && Operators.CompareString(array2[3].Trim(), string.Empty, true) != 0)
{
    text4 = General.ImportValidationToneConversion_B(array2[3].Trim());
    if (Operators.CompareString(text4, "Off", true) != 0 && Operators.CompareString(text4, "Search", true) != 0)
    {
        if (text4.Contains(".")) { Main.ScannerData1.Tone = "C " + text4.Trim(); }   // CTCSS
        else                     { Main.ScannerData1.Tone = "D " + text4.Trim(); }   // DCS
    }
    else { Main.ScannerData1.Tone = text4.Trim(); }
}
```

### 5.3 DMA — digital code (`GLG[11]`), only when `GLG[3]` is absent

```csharp
// FrmMain.cs:21915
else if (array2.Length > 11)
{
    text4 = array2[11].Replace("NONE", string.Empty).Trim();
    if (Operators.CompareString(text4, string.Empty, true) != 0 && Common_PS_RF.IsNumericHexStr(text4)
        && text4.Length != 1 && text4.Length != 2 && text4.Length != 3 && text4.Length == 4)
    {
        if (text4.StartsWith("10"))          // DMR colour code 10xx
        {
            Main.ScannerData1.Tone = "CC " + text4.Substring(3, 1).Replace("A", "10").Replace("B", "11")
                .Replace("C", "12").Replace("D", "13").Replace("E", "14").Replace("F", "15");
        }
        else if (text4.StartsWith("8"))      // RAN / RAS / ?
        {
            if (Common_PS_RF.IsNumericStr(text4))
            {
                int num = Conversions.ToInteger(text4);
                if (num > 8191 && num < 8256)      { Main.ScannerData1.Tone = "R " + Conversions.ToString(num - 8192); }  // RAN
                else if (num == 8190 || num == 8256) { Main.ScannerData1.Tone = "A " + Conversions.ToString(0); }
                else if (num == 8191 || num == 8257) { Main.ScannerData1.Tone = "A " + Conversions.ToString(1); }
                else                                 { Main.ScannerData1.Tone = "U1 " + text4; }
            }
            else { Main.ScannerData1.Tone = "U2 " + text4; }
        }
        else { Main.ScannerData1.Tone = "U3 " + text4; }
    }
}
```

The `Length == 4` guard with three explicit `!= 1/2/3` exclusions is effectively `Length == 4`
(and the first three tests are redundant) — decompiler-faithful copy of the original.

### 5.4 DMA — P25 NAC from the STS screen glyphs

```csharp
// FrmMain.cs:21963
if (Main.ScannerData1.Activity && Operators.CompareString(Main.ScannerData1.Tone, string.Empty, true) == 0)
{
    charPosRev = array[7].IndexOf(Conversions.ToString(Strings.Chr(212)) + Conversions.ToString(Strings.Chr(213)) + Conversions.ToString(Strings.Chr(214)));
    if (charPosRev > -1)
    {
        text4 = array[7].Substring(charPosRev + 3).Trim();
        if (Operators.CompareString(text4, string.Empty, true) != 0 && Common_PS_RF.IsNumericHexStr(text4))
        {
            Main.ScannerData1.Tone = "N " + text4;
        }
    }
}
```

So for DMA the **`Tone` string itself is overloaded**: `C ` = CTCSS, `D ` = DCS, `N ` = P25 NAC,
`CC ` = DMR colour code, `R ` = RAN, `A ` = 0/1, `U1/U2/U3` = unrecognised. The NAC path only runs
when `Tone` is still empty (analog and colour-code paths take priority). There is also a
screen-glyph analog fallback in the frequency-recovery block, which writes `C ` / `D ` from
`array[7]` after stripping non-`[0-9A-Z.]` characters (`FrmMain.cs:22135-22146`).

### 5.5 HP/SDS — `DigitalStatus` comes from the XML side-channel

`ProcessRXData_Uniden_Common_HP` calls `ExtractXMLData` first (`FrmMain.cs:23322`), which walks the
`GSI,1` XML payload. Digital status is a `<Property P25Status=…>` attribute; mute, squelch and RSSI
are also XML attributes:

```csharp
// FrmMain.cs:24072
if (Operators.CompareString(name16, "P25Status", true) == 0)
{
    Main.ScannerData1.DigitalStatus = xmlReader.Value.Replace("None", string.Empty).Trim();   // FrmMain.cs:24077
}
else if (Operators.CompareString(name16, "Mute", true) == 0)
{
    if (Operators.CompareString(xmlReader.Value, "Unmute", true) == 0) { UnMute = true; }      // :24083
}
else if (Operators.CompareString(name16, "SQL", true) == 0)  { SQL  = xmlReader.Value; }       // :24088
else if (Operators.CompareString(name16, "VOL", true) == 0)  { VOL  = xmlReader.Value; }       // :24092
...
Rssi = xmlReader.Value.Trim();                                                                 // :24117
```

and `Enc` is then used as a proxy for channel activity (a muted/encrypted talkgroup still counts):

```csharp
// FrmMain.cs:23330
if (Operators.CompareString(Main.ScannerData1.DigitalStatus, "Enc", true) == 0)
{
    Main.ScannerData1.Activity = true;
}
```

`DigitalStatus` is persisted into recordings (`FileRecording.cs:493`, `FileRecordingUID.cs:490`)
and history rows (`FrmMain.cs:25603-25605`, `:26885`).

### 5.6 Class 5 (DN/125AT family) — a third scheme

Not DMA, not HP/SDS: `DigitalStatus` is sliced out of the STS screen text, and the DMR slot comes
from an STS token, while `Tone` uses the same `GLG` indices as DMA:

```csharp
// FrmMain.cs:23068 — DMRSlot from STS array[7]
empty = array[7].Trim();
if (Operators.CompareString(empty, "Slot1", true) == 0 || Operators.CompareString(empty, "Slot2", true) == 0)
    _0024STATIC_0024…slot = "/" + empty.Replace("Slot", string.Empty);
...
// FrmMain.cs:23077 — DigitalStatus from STS array[9], chars 7..9
if (array.Length > 9 && array[9].Length > 10 && … Substring(5,1) blank … Substring(6,3) non-blank … Substring(9,1) blank …)
{
    Main.ScannerData1.DigitalStatus = array[9].Substring(6, 3);
}
```

`ProcessRXData_Uniden_125` (`FrmMain.cs:22544`) uses the same `GLG[3]` tone logic but
`GLG[11]`-style codes are read from `array2[3]` (`FrmMain.cs:22586-22605`) — i.e. the DN/125AT
screens put the digital code where the DMA family puts the analog tone.

### 5.7 Summary for the requesting app

| | DMA (class 2) | HP/SDS (classes 3/4) | DN/125AT (class 5) |
|---|---|---|---|
| Analog tone | `GLG[3]` → `"C "`/`"D "` | *not parsed* | `GLG[3]` |
| Digital code | `GLG[11]` → `"CC "`/`"R "`/`"A "`/`"U* "` | *not parsed* | STS `array[9]` slice |
| P25 NAC | STS `array[7]`, after `Chr(212)+Chr(213)+Chr(214)` → `"N "` | *not parsed* | *not parsed* |
| `DigitalStatus` | **never set** | XML `Property P25Status` | STS `array[9].Substring(6,3)` |
| DMR slot | never set | XML `RecSlot` | STS `array[7]` == `Slot1`/`Slot2` |

---

## Appendix — search terms used for the "NOT FOUND" claims

| Claim | Terms searched (all `*.cs`) |
|---|---|
| No `GOM` command | `"GOM`, `GOM\r`, `GOM,`, `GOM` |
| No `MSI` command | `"MSI`, `MSI\r`, `MSI,` |
| No MDL→enum translation table | `Enum.Parse(typeof(Main.RadioType)`, `Main.ScannerType`, `ScannerType.Contains`, `CompareString(Main.ScannerType`, `"BCD996P2"`, `"SDS100"` (string literals) |
| `GLG[2]` / `GLG[8]` unused by DMA | awk scan of `FrmMain.cs:21800-22544` for `array2[` — only indices 0,1,3,4,5,6,7,9,10,11 appear |
| DMA never sets `DigitalStatus` | awk scan of `FrmMain.cs:21800-22544` for `DigitalStatus` — zero hits; assignments exist only at `:21288` (clear), `:23082` (class 5), `:24077` (XML) |
| HP/SDS never sets `Tone` | awk scan of `FrmMain.cs:23279-23578` for `ScannerData1.Tone` — zero hits |
