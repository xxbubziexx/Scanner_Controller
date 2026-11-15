# ProScan custom formatters + recording filename scheme

Reverse-engineered from `ProScan.exe` (VB.NET, .NET 4.8, unobfuscated). Every claim carries
its provenance as `file:line` in `decompiled/` (ILSpy 7.2.1 output).

Source files:
- `General.cs:33805` — `GetFormatters()`, the canonical formatter
- `Common_PS_RF.cs:784` / `:801` — the **second, parallel** formatter (streaming metadata)
- `Recorder.cs:1112` — `GetFileName()`, the recording filename assembly
- `Recorder.cs:1204` / `:1220` — folder and filename sanitizers
- `General.cs:34028` / `:34066` — the character allowlists
- `Common_PS_RF.cs:330-348` — time/date helpers
- `FrmOptions.cs`, `FrmSelectMetadata.cs` — where the templates are edited

---

## 0. Read this first: there are TWO formatters, not one

The same `%XX` syntax is implemented **twice**, with **different specifier sets**:

| | Engine A | Engine B |
|---|---|---|
| Function | `General.GetFormatters` (`General.cs:33805`) | inline chain, `Common_PS_RF.cs:784` + `:801` |
| Drives | filename, folder, window caption, MP3/WAV tags | streamed metadata (Source Client / Web Server) |
| Specifiers | **37** | **19** |
| Date/time specifiers | yes (`%D %DD %DT %TT %M %MM %Y %YY %H`) | **none** |
| `%CL` (listener count) | **absent** | present |
| `%TA` / `%TB` / `%P` / `%AF1` / `%AF2` / `%ST*` / `%AC` / `%SC` | present | **absent** |

**Consequence:** `%D`, `%DT`, `%C` etc. work in a filename; `%DT` does **not** work in
streamed metadata (it passes through literally), and `%CL` works in metadata but **not** in a
filename. They are not interchangeable. See §4.

---

## 1. Engine A — `General.GetFormatters` (`General.cs:33805`)

```csharp
public static string GetFormatters(string CustomFormat, DateTime Time)
{
    if (Operators.CompareString(CustomFormat, (string)null, true) == 0)
    {
        CustomFormat = string.Empty;
    }
    if (DateTime.Compare(Time, DateTime.MinValue) == 0)
    {
        Time = DateAndTime.get_Now();
    }
    return CustomFormat.Replace("%FT", Common_PS_RF.GetFreqTGID1())
        .Replace("%TG", Main.ScannerData1.TalkGroup)
        .Replace("%L2", Main.ScannerData1.SystemName)
        .Replace("%L1", Main.ScannerData1.ChannelName)
        .Replace("%ST1", Main.ScannerData1.ServiceType)
        .Replace("%STC", Main.ScannerData1.ServiceColor)
        .Replace("%AC",  Main.ScannerData1.AlertColor)
        .Replace("%SC",  Main.InfoSCState)
        .Replace("%ST",  Main._RadioType.ToString())
        .Replace("%DD",  Time.Date.ToString("dd"))
        .Replace("%DT",  Common_PS_RF.GetCurrentDateTime(1, Time))
        .Replace("%TT",  Common_PS_RF.GetCurrentTime(Time))
        .Replace("%H",   Time.ToString("HH"))
        .Replace("%MM",  Time.Date.ToString("MMM"))
        .Replace("%YY",  Time.Date.ToString("yyyy"))
        .Replace("%AF1", Main.ProgramPath)
        .Replace("%AF2", Path.GetFileName(Main.ProgramPath))
        .Replace("%FA",  Main.ScannerData1.FavoriteName)
        .Replace("%SI",  Main.ScannerData1.SiteName)
        .Replace("%S",   Main.ScannerData1.SystemName)
        .Replace("%G",   Main.ScannerData1.GroupName)
        .Replace("%B",   Main.ScannerData1.GroupName)
        .Replace("%DE",  Main.ScannerData1.GroupName)
        .Replace("%C",   Main.ScannerData1.ChannelName)
        .Replace("%F",   Main.ScannerData1.Frequency + Main.ScannerData1.DMRSlot)
        .Replace("%TA",  Main.ScannerData1.FTOA)
        .Replace("%TB",  Main.ScannerData1.FTOB)
        .Replace("%T",   Main.ScannerData1.Tone)
        .Replace("%MD",  Main.ScannerData1.Modulation)
        .Replace("%R",   (!Main.RssiMeterBars) ? Main.ScannerData1.Rssi1 : Main.ScannerData1.Rssi2)
        .Replace("%U1",  Main.ScannerData1.UID1)
        .Replace("%U2",  Main.ScannerData1.UID2)
        .Replace("%U3",  Main.ScannerData1.UID3)
        .Replace("%P",   Main.CommPort)
        .Replace("%D",   Time.Date.ToString(Main.MonthDayYearFormat))
        .Replace("%M",   Time.Date.ToString("MM"))
        .Replace("%Y",   Time.Date.ToString("yy"));
}
```

### Semantics

- **Chained `String.Replace`.** Not a parser. Each specifier is substituted sequentially over
  the *result* of the previous pass — the output of one replacement is visible to every later
  one. This is the source of both the ordering guarantees (§2) and the injection hazard (§3).
- **`null` format → `string.Empty`.** Null is coerced, not thrown.
- **`DateTime.MinValue` → "now".** Passing `MinValue` is the idiomatic "use current time"
  signal. Both call styles exist in the codebase.
- **Case-sensitive.** `.NET String.Replace` is ordinal — `%tg` is left untouched. Only the
  exact uppercase spellings work.
- **No escape mechanism.** There is no `%%` or quoting. A literal `%F` in your template is
  indistinguishable from the specifier.
- **No sanitisation.** `GetFormatters` does not filter characters; callers do (§5, §6).

### The specifier table

Grouped by purpose. "Source field" is the `Main.ScannerData1` (or `Main`) member read.

**Scanner / channel data**

| Spec | Expands to | Source field | Notes |
|---|---|---|---|
| `%FT` | frequency **or** talkgroup | `GetFreqTGID1()` | talkgroup if set, else `Frequency + DMRSlot`; **empty if not receiving** |
| `%TG` | talkgroup | `TalkGroup` | |
| `%F` | frequency (+ DMR slot) | `Frequency + DMRSlot` | always the frequency; DMR slot **concatenated with no separator** (`851.3375S`) |
| `%L1` | channel name | `ChannelName` | |
| `%L2` | system name | `SystemName` | |
| `%C` | channel name | `ChannelName` | `%C` and `%L1` are the **same field** |
| `%S` | system name | `SystemName` | `%S` and `%L2` are the **same field** |
| `%SI` | site name | `SiteName` | |
| `%FA` | favorite list name | `FavoriteName` | |
| `%G`, `%B`, `%DE` | group/department | `GroupName` | **all three are the same field** |
| `%T` | tone | `Tone` | |
| `%TA` / `%TB` | FTO A / FTO B | `FTOA` / `FTOB` | |
| `%MD` | modulation | `Modulation` | |
| `%U1` `%U2` `%U3` | unit IDs | `UID1/2/3` | |
| `%R` | RSSI | `Rssi1` or `Rssi2` | picks `Rssi2` when `RssiMeterBars` is on |
| `%ST1` | service type | `ServiceType` | |
| `%STC` | service colour | `ServiceColor` | |
| `%AC` | alert colour | `AlertColor` | |
| `%SC` | (feeds Source Client state) | `Main.InfoSCState` | not a channel field |

**Time / date**

| Spec | Expands to | Implementation |
|---|---|---|
| `%D` | full date | `Time.Date.ToString(Main.MonthDayYearFormat)` |
| `%DD` | day of month, 2-digit | `Time.Date.ToString("dd")` |
| `%M` | month **number**, 2-digit | `Time.Date.ToString("MM")` |
| `%MM` | month **name**, abbreviated | `Time.Date.ToString("MMM")` |
| `%Y` | year, 2-digit | `Time.Date.ToString("yy")` |
| `%YY` | year, 4-digit | `Time.Date.ToString("yyyy")` |
| `%H` | hour, 24-hour 2-digit | `Time.ToString("HH")` |
| `%TT` | time only | `GetCurrentTime(Time)` → `GetTimeFormat()` |
| `%DT` | full date + space + time | `GetCurrentDateTime(1, Time)` |

**Application**

| Spec | Expands to |
|---|---|
| `%ST` | current scanner model (`Main._RadioType`), e.g. `BCD996XT` |
| `%P` | current COM port (**bare digits**, e.g. `3`, per the port doc) |
| `%AF1` | full program path |
| `%AF2` | program filename only (`Path.GetFileName`) |

> ### ⚠ Three counterintuitive inversions — memorise these
>
> - **`%MM` is the month NAME (`MMM` → `Sep`); `%M` is the month NUMBER (`MM` → `09`).**
>   The doubled one is *not* the numeric one.
> - **`%YY` is the 4-DIGIT year (`yyyy` → `2026`); `%Y` is the 2-DIGIT year (`yy` → `26`).**
>   Doubling makes it *longer*, the opposite of `.NET` where more `y` = more digits.
> - **`%D` is the FULL date; `%DD` is only the 2-digit DAY.** This breaks the "doubled = longer"
>   rule that `%M`/`%MM` and `%Y`/`%YY` follow. `%D` and `%DD` are unrelated in output.
>
> Getting these backwards produces silently wrong filenames — a `%MM` you expected to be `09`
> yields `Sep`, and a `%Y` you expected to be `2026` yields `26`.

---

## 2. Replacement ordering is deliberate and correct

The chain is ordered **longest-specifier-first wherever two specifiers share a prefix**, and I
verified every such pair:

| Pair | Order in chain | Safe? |
|---|---|---|
| `%FT` before `%F` | 1 → 25 | ✓ |
| `%TG` before `%T` | 2 → 28 | ✓ |
| `%ST1`, `%STC` before `%ST`, before `%S` | 5, 6 → 9 → 20 | ✓ |
| `%SC` before `%S` | 8 → 20 | ✓ |
| `%DD`, `%DT` before `%D` | 10, 11 → 35 | ✓ |
| `%TT`, `%TA`, `%TB` before `%T` | 12, 26, 27 → 28 | ✓ |
| `%MM` before `%M` | 14 → 36 | ✓ |
| `%YY` before `%Y` | 15 → 37 | ✓ |
| `%FA` before `%F` | 18 → 25 | ✓ |
| `%SI` before `%S` | 19 → 20 | ✓ |
| `%C` before nothing longer | — | ✓ (`%CL` is not in this engine) |
| `%MD` vs `%M`/`%MM` | 29, after `%MM`(14), before `%M`(36) | ✓ (no shared prefix conflict) |

So the ordering is **right**. If you reimplement this or add a specifier, you must preserve
longest-first or `%ST` will eat the `%ST1` in `%ST1C` etc.

The one hazard the ordering cannot protect against is in §3.

---

## 3. No escaping → data-driven specifier injection

Because each pass sees the **output** of the previous pass, and because `%` is an allowed
character in both the filename and folder allowlists (§6), a **value read from the scanner or
database** that contains a `%X` sequence will be substituted by a later pass.

Worked example — with the default recording format `%DT - %S - %C`, suppose the channel name is
literally `TAC %T`:

1. `%DT` → `09/23/26 15:30:00`
2. … `%C` (position 24) → `TAC %T`
3. `%F` (25) → nothing to do
4. `%TA`/`%TB` (26, 27) → no match
5. **`%T` (28) → the channel name's literal `%T` is replaced with the tone** (probably empty)

Result: the channel name is silently mangled. The same applies to `%S`, `%C`, `%F`, `%M`, `%D`,
`%Y`, `%P`, `%R`, `%U1`… in any field that gets substituted *before* them.

Practical impact: rare in practice (real channel names rarely contain `%`), but it means
**formatter output is not idempotent and not injective**. If your tool round-trips ProScan
templates, do not assume a literal `%` survives — and don't generate templates from untrusted
scanner text.

Note that `%FT` is replaced **first** (position 1), so `GetFreqTGID1()`'s return value *is*
subject to every later substitution. It returns a frequency or talkgroup, so the risk is low,
but it is the widest-exposure field in the chain.

---

## 4. Engine B — the streaming-metadata formatter (`Common_PS_RF.cs:784`, `:801`)

A hand-rolled **duplicate** chain inside `GetMetaData()`, applied to `Main.MetadataCustomFormat`.
It exists twice, once per consumer, differing only in the listener-count source:

```csharp
// :784  Source Client (SC) — %CL = Main.CurrentListenersSC
// :801  Web Server (WS)    — %CL = Main.CurrentListenersWS
Main.MetadataCustomFormat
    .Replace("%FT", GetFreqTGID1())
    .Replace("%TG", Main.ScannerData1.TalkGroup)
    .Replace("%L2", Main.ScannerData1.SystemName)
    .Replace("%L1", Main.ScannerData1.ChannelName)
    .Replace("%FA", Main.ScannerData1.FavoriteName)
    .Replace("%SI", Main.ScannerData1.SiteName)
    .Replace("%S",  Main.ScannerData1.SystemName)
    .Replace("%G",  Main.ScannerData1.GroupName)
    .Replace("%B",  Main.ScannerData1.GroupName)
    .Replace("%DE", Main.ScannerData1.GroupName)
    .Replace("%CL", Main.CurrentListenersSC.ToString())   // or CurrentListenersWS
    .Replace("%C",  Main.ScannerData1.ChannelName)
    .Replace("%F",  Main.ScannerData1.Frequency + Main.ScannerData1.DMRSlot)
    .Replace("%T",  Main.ScannerData1.Tone)
    .Replace("%MD", Main.ScannerData1.Modulation)
    .Replace("%R",  newValue)                              // Rssi1 / Rssi2
    .Replace("%U1", Main.ScannerData1.UID1)
    .Replace("%U2", Main.ScannerData1.UID2)
    .Replace("%U3", Main.ScannerData1.UID3);
```

**Specifiers that DO NOT WORK in streamed metadata** (present in Engine A only):
`%DD %DT %TT %H %MM %YY %M %Y %D %ST %ST1 %STC %AC %SC %P %AF1 %AF2 %TA %TB`

They pass through **literally** into the streamed `Artist`/`Title` field. So a metadata template
of `%DT %C` streams the literal text `%DT FIRE-TAC1`. If you want a timestamp in streamed
metadata, ProScan cannot give you one — that is an Engine A-only capability.

**Specifier that works ONLY here:** `%CL` — current listener count, `SC` vs `WS` depending on
which of the two chains you're looking at. Not available in filenames.

Ordering is again longest-first (`%CL` before `%C` at 792→793 and 809→810) and correct.

Two more behaviours in the same function:
- **Broadcast Lockout overrides the whole format.** If `Main.BroadcastLockoutFlagSC` (or `WS`)
  is set, the output is replaced by the literal `"Broadcast Lockout"` (`:818-825`), discarding
  the formatted text entirely.
- **When the scanner is idle** and `MetaDataShowScanningTag` is off, the **last active
  transmission's** text is re-sent (`:829-833`) rather than being blanked — so the streamed
  metadata goes stale rather than empty. With the flag on, it emits `Main.MetaDataScanningTag`.

---

## 5. Consumers — what feeds each template

| Template | Field | Where formatted | Line |
|---|---|---|---|
| `Main.RecordCustomFormat` | recording **filename stem** | `Recorder.GetFileName` | `Recorder.cs:1117` |
| `Main.RecordingOrganize` | recording **subfolder path** | `Recorder.StartRecord` | `Recorder.cs:173` |
| `Main.RecordUIDCustomFormat` | UID-recording **filename stem** | `RecorderUID` | `RecorderUID.cs:1033` |
| `Main.RecordingUIDOrganize` | UID-recording **subfolder path** | `RecorderUID` | `RecorderUID.cs:170` |
| `Main.CaptionCustomFormat` | **window caption** | `FrmMain` | `FrmMain.cs:27715` |
| `Main.RecTagMp3TIT2Tag` | MP3 **`TIT2`** (Title) | `FileRecording` | `FileRecording.cs:353` |
| `Main.RecTagMp3TPE1Tag` | MP3 **`TPE1`** (Artist) | `FileRecording` | `:354` |
| `Main.RecTagMp3TPE2Tag` | MP3 **`TPE2`** (Band) | `FileRecording` | `:355` |
| `Main.RecTagWavINAMTag` | WAV **`INAM`** (Title) | `FileRecording` | `:422` |
| `Main.RecTagWavIARTTag` | WAV **`IART`** (Artist) | `FileRecording` | `:423` |
| `Main.MetadataCustomFormat` | streamed metadata | **Engine B** | `Common_PS_RF.cs:784/801` |

**Timing — tags are written once, at recording START.** `WriteMP3ID3Header()` is called **only**
from `OpenFileMP3()` (`FileRecording.cs:179`), which `StartRecord` reaches via `:96`. It is never
called again. The three tag templates are evaluated **once, at file creation**, against
`Main.ScannerData1` as it stands at that instant, and then frozen for the life of the file.

So the filename, the folder, and the ID3/WAV tags **all use the recording start time** —
they do *not* differ. `DateAndTime.get_Now()` inside `WriteMP3ID3Header` (`:353-355`) is the
start moment, not the close moment.

**What IS updated at close** is a different thing: `CloseFileprosData` (`FileRecording.cs:812`)
patches only the four placeholder fields *inside the `pros` chunk* — `EndingDate`, `Tone`,
`UID`, `UID#`. It does that through a separate `BinaryWriter` that seeks to the field, writes
space padding to the field's full width, then writes the final value (`:823-832`, `:839-848`,
`:855-864`). The ID3 text frames (`TSEE`, `TRDA`, `TIT2`, `TPE1`, `TPE2`) are **not** touched at
close.

Consequence: if one file spans several transmissions, `TIT2` names only the **first** one, while
the `pros` chunk's Tone/UID get the **last** values. That asymmetry is real and worth designing
around.

### Worked example — `%TG %G %C` in the `TIT2` template

The UI field shown in Options → **Recording Text Tags** → MP3 Files → `TIT2 (Title)`.

| Spec | Source field | Position in chain | Aliases |
|---|---|---|---|
| `%TG` | `ScannerData1.TalkGroup` | 2 | — |
| `%G` | `ScannerData1.GroupName` (department) | 21 | `%B`, `%DE` |
| `%C` | `ScannerData1.ChannelName` | 24 | `%L1` |

With `TalkGroup=1201`, `GroupName="Fire Dispatch"`, `ChannelName="FIRE-TAC1"`:

```
TIT2 = "1201 Fire Dispatch FIRE-TAC1"
```

Four behaviours that matter for this specific template:

1. **Only `TIT2`/`TPE1`/`TPE2` gate the tag** — an OR across the three. Filling `TIT2` alone is
   enough. Clearing all three drops the ID3 tag *and* the `pros` chunk. WAV is ungated — see
   `RECORDING-METADATA-FORMAT.md` §2.
2. **Whitespace is NOT collapsed.** `WriteMP3TextFrame` calls `.Trim()` on the whole string only
   (`FileRecording.cs:377`). An empty middle specifier leaves a double space — e.g. empty
   `GroupName` yields `"1201  FIRE-TAC1"`. Normalise yourself if you parse these back.
3. **An empty field can leave a leading/trailing space** that `.Trim()` removes, so `%TG` empty
   gives `"Fire Dispatch FIRE-TAC1"` — but interior gaps survive. A template whose *only*
   specifier is empty renders as an empty frame, not an error.
4. **Replacement order can corrupt data.** `%G` (21) is expanded before `%C` (24), so a literal
   `%C` inside the GroupName value would be replaced by the channel name. No escaping exists.

The `%DT`/`%TT`/`%H` date-time specifiers also work here, and resolve to the **recording start**
time (see the timing note above), even though `WriteMP3ID3Header` reads `DateAndTime.get_Now()`
at that moment.

The **caption** passes `Main.ScannerData1.Activity ? Main.HistoryStartTime : DateAndTime.get_Now()`
(`FrmMain.cs:27715`) — during a transmission it shows the transmission start, otherwise now.

---

## 6. Recording filename scheme

### Assembly — `Recorder.GetFileName` (`Recorder.cs:1112`)

```csharp
private string GetFileName(string StartDateTime, DateTime RecorderStartTime)
{
    string text = string.Empty;
    if (Main.RecorderMode == 1 || Main.RecorderMode == 2)
    {
        text = General.GetFormatters(Main.RecordCustomFormat, RecorderStartTime).Trim();
        if (text.Trim() == "") { text = "No Custom Formatters Specified"; }
    }
    else if (Main.RecorderMode == 3)
    {
        text = "VOX - " + StartDateTime;          // formatter BYPASSED entirely
    }
    text = FilterFileNamesLegalCharactersOnly(text);
    if (Main.AudioFormatREC == 1)      { text += ".wav"; }
    else if (Main.AudioFormatREC == 2) { text += ".mp3"; }
    return text;
}
```

Order of operations: **format → `.Trim()` → empty-fallback → sanitize → append extension.**

- **Recorder modes 1 and 2** use the template.
- **Mode 3 (VOX) bypasses the formatter completely** — `RecordingCustomFormat` is ignored and
  the name is hardcoded `VOX - <StartDateTime>`. No specifiers apply.
- **Empty template → the literal filename `No Custom Formatters Specified`**, not an error and
  not a fallback pattern. Easy to spot on disk; if you see it, the template is blank.
- The extension is appended **after** sanitisation, so the sanitizer never strips its own dot.

### Collision suffix — and its bug (`Recorder.cs:1097-1109`)

When the target file already exists:

```csharp
int num = 2;
while (true)
{
    text = Filename.Insert(Filename.LastIndexOf("."), " #" + num.ToString());
    if (!File.Exists(Path + text)) { break; }
    num++;
}
```

It inserts ` #2`, ` #3`… at **`LastIndexOf(".")`** — assuming the last dot in the filename is the
extension separator. **That assumption breaks whenever a specifier expands to a value containing
a dot — and `%F` (frequency) always does.**

| Template result | Collision name |
|---|---|
| `FIRE-TAC1.mp3` | `FIRE-TAC1 #2.mp3` ✓ correct |
| `851.3375 - FIRE-TAC1.mp3` | **`851 #2.3375 - FIRE-TAC1.mp3`** ✗ wrong |

The suffix lands inside the frequency instead of before the extension. The file is still valid
and still `.mp3`-terminated, but the numbering is in the wrong place and `851 #2.3375...` sorts
and globs wrongly. Since `%F` is one of the most useful specifiers, this is hit easily. Worth
knowing before you write a parser that expects `name #n.ext`.

### Sanitizers

`FilterFileNamesLegalCharactersOnly` (`Recorder.cs:1220`):

```csharp
Path = Path.Replace("/", "-").Replace(":", "-");
Path = General.RemoveFileNamesNonLegalCharactersOnly(Path).Trim();
while (Path.EndsWith(".")) { Path = Path.Remove(Path.Length - 1); }
```

`FilterFolderLegalCharactersOnly` (`Recorder.cs:1204`):

```csharp
Path = Path.Replace("/", "-");
Path = General.RemoveFolderNonLegalCharactersOnly(Path).Trim();
Path = Strings.Mid(Path, 1, 3) + Strings.Mid(Path, 4).Replace(":", "-");  // drive-letter guard
while (Path.EndsWith(".")) { Path = Path.Remove(Path.Length - 1); }
if (!Path.EndsWith("\\")) { Path += "\\"; }
```

The **`Mid(Path,1,3)` trick** exists to protect a literal drive prefix: it keeps the first three
characters (`C:\`) verbatim, then removes colons from everything after. Without it, a
`RecordingFolder` of `C:\Recordings` would have its drive colon stripped. Everything after
character 3 gets `:` → `-`, which is what turns `%D` (default `MM/dd/yy`, i.e. `09/23/26`, with
`/` already replaced by `-`) into the folder-safe `09-23-26`.

### The character allowlists (`General.cs:34028`, `:34066`)

Both are **allowlists**, applied char-by-char, case-**insensitive** membership
(`CompareMethod.Text`):

```
Folder  (RemoveFolderNonLegalCharactersOnly, :34031)
  ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz1234567890!@#$%^&()-_+={}[];,. :\

File    (RemoveFileNamesNonLegalCharactersOnly, :34069)
  ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz1234567890!@#$%^&()-_+={}[];,. 
```

The **only difference is the trailing `\`** — the folder allowlist permits path separators, the
filename allowlist does not. Everything else is identical, so:

- **Excluded** (Windows-illegal, correctly): `* ? < > | " /` and control characters.
- **Included and notable**: `%` (the injection surface from §3), `;`, `,`, space, and **`:` for
  files** — but `:` was already rewritten to `-` one step earlier, so it can never actually
  appear.
- Characters ≥ 127 (non-ASCII, e.g. accented letters) are **stripped**, since they're absent
  from the allowlist. A channel name with `é` loses the character silently.

> **Cross-check with the tag writer.** The MP3/WAV tags use a *different* filter —
> `General.CleanASCII_1` (`General.cs:35949`), which keeps chars with `Asc > 31 && Asc < 127`,
> i.e. printable ASCII only, plus `.Trim()` and a 253-char cap. So a filename and an ID3 tag
> built from the same template can differ: the allowlist strips `*?<>|"`, while `CleanASCII_1`
> strips control characters and everything non-ASCII. Neither is a superset of the other.

---

## 7. Time/date helpers (`Common_PS_RF.cs:330-348`)

```csharp
public static string GetTimeFormat()   // :346
    => Main.MilitaryTime ? "HH:mm:ss" : "h:mm:ss tt";

public static string GetCurrentTime(DateTime NowDateTime)            // :330
    => Strings.Format(NowDateTime, GetTimeFormat());

public static string GetCurrentDateTime(int Spaces, DateTime NowDateTime)   // :335
    => Strings.Format(NowDateTime, Main.MonthDayYearFormat)
     + Strings.Space(Spaces)
     + Strings.Format(NowDateTime, GetTimeFormat());
```

- `%TT` = time under `GetTimeFormat()`, which is **24-hour (`HH:mm:ss`) when `MilitaryTime` is
  on, 12-hour with an AM/PM suffix (`h:mm:ss tt`) when off**. The single most likely reason a
  template's output doesn't match expectations.
- `%DT` = formatted date, then **exactly one space** (`Spaces = 1` is hardcoded at the call
  site, `General.cs:33823`), then the time.
- **Two different formatting engines are in play.** `%DT`/`%TT` go through VB's
  `Strings.Format`; `%D`, `%DD`, `%M`, `%MM`, `%Y`, `%YY`, `%H` go through .NET
  `DateTime.ToString`. For the default `MM/dd/yy` they agree, but they are not the same
  implementation and can diverge on unusual `MonthDayYearFormat` values.
- `Main.MonthDayYearFormat` default is **`"MM/dd/yy"`** (`General.cs:7639`), user-editable via a
  combo box (`FrmOptions.cs:8251`). It's reused for certificate dates too (`Certificates.cs:267`).

---

## 8. Defaults and where to edit them

Factory defaults (`General.cs`):

| Setting | Default | Line |
|---|---|---|
| `RecordCustomFormat` | `%DT - %S - %C` | `:6884` |
| `RecordingOrganize` | `%D\%S` | `:6876` |
| `RecordUIDCustomFormat` | `%S - UID: %U1` | `:7951` |
| `RecordingUIDOrganize` | `%D\%S` | `:7943` |
| `CaptionCustomFormat` | `%ST - %S - %C` | `:6828` |
| `MetadataCustomFormat` | `%C` | `:7180` |
| `MonthDayYearFormat` | `MM/dd/yy` | `:7639` |
| `RecTagMp3TIT2Tag` / `TPE1` / `TPE2` | **empty** | `:7651`+ |
| `RecTagWavINAMTag` / `IART` | **empty** | `:7679`+ |

Note the **MP3/WAV tag templates default to empty** — which is exactly why the ID3 tag isn't
written at all by default (`FileRecording.cs:337`: the tag is only emitted if at least one of
TIT2/TPE1/TPE2 is non-empty). Consistent with `RECORDING-METADATA-FORMAT.md` §2.

Defaults resolve to:
- recording filename: `09/23/26 15:30:00 - County P25 - FIRE-TAC1` → sanitized (date `/`→`-`) →
  `09-23-26 15:30:00 - County P25 - FIRE-TAC1.mp3`
- folder: `%D\%S` → `09-23-26\County P25\` appended to `RecordingFolder`
- caption: `BCD996XT - County P25 - FIRE-TAC1`

UI locations (`FrmOptions.cs` / `FrmSelectMetadata.cs`):

| Template | Control |
|---|---|
| `RecordingOrganize` | `TextBox1` (`:8566`) |
| `RecordCustomFormat` | `TextBox2` (`:8574`) |
| `CaptionCustomFormat` | `TextBox3` (`:8151`) |
| `RecordingUIDOrganize` | `TextBox11` (`:8578`) |
| `RecordUIDCustomFormat` | `TextBox12` (`:8586`) |
| MP3 `TIT2` | `TextBox4` (`:8734`) |
| WAV `INAM` | `TextBox8` (`:8762`) |
| `MonthDayYearFormat` | `ComboBox25` (`:8251`) |
| `MetadataCustomFormat` | `FrmSelectMetadata.TextBox1` (`:1712`) |

### Config keys (`ProScan.cfg`) and the `BLANK` sentinel

| Key | Field |
|---|---|
| `[CUSTOM RECORDING FORMAT]` | `RecordCustomFormat` |
| `[RECORDING ORGANIZE]` | `RecordingOrganize` |
| `[UID CUSTOM RECORDING FORMAT]` | `RecordUIDCustomFormat` |
| `[UID RECORDING ORGANIZE]` | `RecordingUIDOrganize` |
| `[CUSTOM CAPTION FORMAT]` | `CaptionCustomFormat` |
| `[SCANCAST METADATA CUSTOM FORMAT]` | `MetadataCustomFormat` |
| `[MONTH DAY YEAR FORMAT]` | `MonthDayYearFormat` |
| `[RECORDINGS MP3 TIT2 TAG]` / `TPE1` / `TPE2` | MP3 tag templates |
| `[RECORDINGS MP3 INAM TAG]` / `[RECORDINGS MP3 IART TAG]` | **WAV** `INAM`/`IART` templates |
| `[RECORDINGS FOLDER]` / `[UID RECORDINGS FOLDER]` | base paths |

> **Quirk — `BLANK` is a sentinel, not an empty value.** An empty template is written as
> `[RECORDING ORGANIZE]=BLANK` (`General.cs:24952-24956`), and the reader maps `"BLANK"`
> (case-insensitive) back to `""` (`General.cs:9439`, `:10542`, `:10589`, `:13028`, `:14109`,
> `:14341`). Writing `BLANK` into this key yourself to mean the literal string `BLANK` is not
> possible.

> **Quirk — the WAV tag keys are labelled "MP3".** The WAV `INAM`/`IART` templates are persisted
> under `[RECORDINGS MP3 INAM TAG]` / `[RECORDINGS MP3 IART TAG]` (`General.cs:26660`, `:26664`)
> while holding `Main.RecTagWavINAMTag` / `RecTagWavIARTTag`. The key names are wrong; the values
> are the WAV ones. Don't key off the name.

---

## 9. Quirks summary

1. **Two engines, different specifier sets.** `%DT` works in filenames, not in streamed
   metadata. `%CL` works in metadata, not in filenames.
2. **`%MM` = month name, `%M` = month number. `%YY` = 4-digit year, `%Y` = 2-digit.**
   `%D` = full date, `%DD` = 2-digit day. All three are counterintuitive.
3. **Aliased specifiers:** `%C`≡`%L1` (channel), `%S`≡`%L2` (system), `%G`≡`%B`≡`%DE` (group).
4. **Chained `Replace`, no escaping** → a literal `%T`/`%S`/`%C` in scanner data gets
   substituted by a later pass; output is not injective.
5. **Case-sensitive** — `%tg` does nothing.
6. **Collision suffix inserts at `LastIndexOf(".")`**, so any dot in the formatted name (i.e.
   any `%F`) puts ` #2` in the wrong place.
7. **VOX (RecorderMode 3) ignores the format entirely** — `VOX - <datetime>`.
8. **Empty template → filename `No Custom Formatters Specified`**, silently.
9. **Tags are evaluated once, at recording start** (`WriteMP3ID3Header` is called only from
    `OpenFileMP3`, `FileRecording.cs:179`). Filename, folder and tags therefore share the *same*
    start timestamp. A file spanning multiple transmissions gets a `TIT2` naming only the
    **first**, while the `pros` chunk's `Tone`/`UID`/`UID#`/`EndingDate` are patched at close with
    the **last** values (`CloseFileprosData`, `FileRecording.cs:812`).
10. **`%TT` flips between 24-hour and 12-hour with the `MilitaryTime` setting.**
11. **Two different period-formatting engines** (VB `Strings.Format` vs .NET `ToString`) behind
    specifiers that look equivalent.
12. **Filename allowlist permits `%`** — the injection surface, and `*?<>|"` are correctly
    excluded; non-ASCII is stripped entirely.
13. **Empty values persist as `BLANK`**; WAV tag keys are mislabelled `MP3`.
14. **Idle behaviour:** streamed metadata goes stale (re-sends last transmission) rather than
    blank, unless `MetaDataShowScanningTag` is set; broadcast lockout overrides the format with
    a literal string.

---

## Verify it yourself

**Verified by execution.** The specifier table in §1 was confirmed by reimplementing the exact
`GetFormatters` chain in C# and running it on .NET 6 with a fixed date (`2026-09-23 15:30`).
Output, from the real .NET/VB formatting engines:

```
%MM  -> Sep        %M  -> 09        %YY -> 2026      %Y  -> 26
%D   -> 09/23/26   %DD -> 23        %H  -> 15
%TT  -> 15:30:00   (MilitaryTime=false -> 3:30:00 PM)
%DT  -> 09/23/26 15:30:00
%F   -> 851.3375S  (DMR slot concatenated, no separator)
'%MM-%M-%YY-%Y' -> Sep-09-2026-26
'%DT - %S - %C' -> 09/23/26 15:30:00 - County P25 - FIRE-TAC1
```

Injection hazard confirmed live: a channel name of `TAC %T` rendered through `%DT - %C` becomes
`09/23/26 15:30:00 - TAC CTCSS 100.0` — the literal `%T` is consumed. Case sensitivity confirmed:
`%tg` passes through untouched, `%TG` resolves.

> **One specifier NOT verified this way: `%AF2`.** The harness ran on Linux, where
> `System.IO.Path.GetFileName(@"C:\ProScan\ProScan.exe")` treats `\` as an ordinary character and
> returns the whole string. On Windows — the only platform ProScan runs on — it returns
> `ProScan.exe`. The behaviour in §1 is correct for Windows; it just cannot be confirmed in a
> Linux harness. Same caveat for anything else routed through `System.IO.Path`.

Everything above is also readable in the decompile:

```bash
cd ~/workspace/proscan/decompiled
sed -n '33805,33850p' General.cs       # Engine A, the full chain + exact ordering
sed -n '784,817p'      Common_PS_RF.cs # Engine B (SC + WS), different specifier set
sed -n '1112,1137p'    Recorder.cs     # GetFileName: format -> fallback -> sanitize -> ext
sed -n '1097,1109p'    Recorder.cs     # the #2 collision bug
sed -n '1204,1229p'    Recorder.cs     # both sanitizers
sed -n '34028,34042p'  General.cs      # folder allowlist
sed -n '34066,34080p'  General.cs      # filename allowlist (differs by one char: \)
sed -n '330,348p'      Common_PS_RF.cs # GetTimeFormat / GetCurrentTime / GetCurrentDateTime
```

Field-check against a real install: set a recording template, record one transmission, and
compare the resulting filename and the ID3 `TIT2` value against this document. The strongest
single check is the `%MM` vs `%M` inversion — put `%MM-%M-%YY-%Y` in the caption format and
read the window title; you should get `Sep-09-2026-26`.
