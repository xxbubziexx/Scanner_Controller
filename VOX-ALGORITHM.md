# ProScan VOX / audio-level squelch — full arithmetic

Source: ILSpy 7.2.1 decompile of ProScan.exe (VB.NET, .NET 4.8) at
`/home/kali/workspace/proscan/decompiled`. Every claim cites `file:line`.
`[PROVEN]` = read directly from the decompiled source. `[INFERENCE]` = derived,
not literally stated. `[NOT FOUND]` = searched and absent.

Names: `leftlist1` == `_0024STATIC_0024ProcessAudio2_002420411D61D6108108_0024leftlist1`.

---

## 1. ENERGY MEASUREMENT

Per input buffer, `ProcessAudio2` splits the interleaved stereo buffer and
accumulates **sum of squares + sample count** per channel — WaveIn.cs:926-946:

```csharp
for (int i = 0; i <= num6; i++)                       // num6 = BufferStereo.Length - 1  (:923)
{
    if (unchecked(i % 2) == 0)
    {
        short num7 = (BufferStereo[i] = Common_PS_RF.LimitShort1(BufferStereo[i], num4));
        _0024STATIC_..._leftbuffer[num8] = num7;
        num9 += Math.Pow(num7, 2.0);
        num10++;
        num8++;
    }
    else
    {
        short num7 = (BufferStereo[i] = Common_PS_RF.LimitShort1(BufferStereo[i], num5));
        _0024STATIC_..._rightbuffer[num11] = num7;
        num12 += Math.Pow(num7, 2.0);
        num13++;
        double a = Common_PS_RF.LimitShort1((double)BufferStereo[i] / 2.0 + (double)BufferStereo[i - 1] / 2.0, level);
        BufferMono[num11] = (short)Math.Round(a);
        num11++;
    }
}
```

`num9` = Σx² (left), `num10` = left sample count; `num12`/`num13` = right.
Both are **call-local** (declared WaveIn.cs:887-890), so they are reset every
buffer — the accumulator never spans buffers. `num4`/`num5` are the post-gain
left/right line-in levels (WaveIn.cs:878-881 for `_Type == 3`), and
`num4 != 0.0` is the gate that enables the whole left metering block
(WaveIn.cs:1002) / `num5 != 0.0` for right (WaveIn.cs:1120) — a muted channel
never updates its meter.

The value appended to the history list — WaveIn.cs:989-993:

```csharp
int item = default(int);
if (num10 > 0 && num9 / (double)num10 * 2.0 >= 0.0)
{
    item = (int)Math.Round(Math.Sqrt(num9 / (double)num10 * 2.0));
}
```

```c
// num9/num10 = mean square over the CURRENT buffer only
item_left  = (num10 > 0) ? round( sqrt( (num9 /num10) * 2 ) ) : 0
item_right = (num13 > 0) ? round( sqrt( (num12/num13) * 2 ) ) : 0   // WaveIn.cs:1109-1113
```

[PROVEN] `item` is **mean square over one buffer, scaled ×2, then square-rooted**.
That is `RMS × √2` — i.e. **peak-equivalent RMS** (for a sine, peak = RMS·√2),
in raw 16-bit sample units (0…32767 nominal, up to ~46341 if the mean square is
near full scale). It is *not* a raw sum of squares (the sum is divided by the
count), *not* a plain RMS (the √2), and *not* a peak (nothing tracks max|x|).

Window = **one capture buffer per call per channel**. Buffer duration =
`_BufferSize / (2 * _SamplesPerSec)` s — the WAVEHDR is `_BufferSize * 2` bytes
(WaveIn.cs:242) of 16-bit **stereo** frames (`nChannels = 2`, WaveIn.cs:200).
For the Recorder stream, `BufferSize = round(SampleRateREC * 0.2)`
(FrmMain.cs:40212) and the default `SampleRateREC = 22050` (General.cs:6912) →
BufferSize 4410 shorts → **0.1 s of audio per item**. So one `item` ≈ 100 ms
energy, not one sample. `_BufferSize` is forced even (WaveIn.cs:190-193).

URL-audio path (`_DeviceID == -2`, WaveIn.cs:948-979) accumulates only the left
channel — `num12`/`num13` are never touched, so `item_right` is always 0 there.

## 2. NOISE FLOOR / HISTORY RING

Append + cap — WaveIn.cs:994-998 (left), :1114-1118 (right):

```csharp
_0024STATIC_..._leftlist1.Add(item);
if (_0024STATIC_..._leftlist1.Count > num3 * 1200)
{
    _0024STATIC_..._leftlist1.RemoveAt(0);
}
```

`num3` — WaveIn.cs:855-865:

```csharp
_0024STATIC_..._intervaltotal += _0024STATIC_..._intervalsw.ElapsedMilliseconds;
_0024STATIC_..._intervalsw.Reset();
_0024STATIC_..._intervalsw.Start();
if (_0024STATIC_..._intervaltotal < 1.0)
{
    return;
}
checked
{
    _0024STATIC_..._intervalcount++;
    int num3 = (int)Math.Round(1000.0 / (_0024STATIC_..._intervaltotal / (double)_0024STATIC_..._intervalcount));
```

```c
num3 = round( 1000.0 / (intervaltotal / intervalcount) )   // buffers (callbacks) per second
```

`intervaltotal` and `intervalcount` are only ever `+=` / `++` (WaveIn.cs:855,
:864) — **never reset** — so `num3` is the *process-lifetime average* buffer
rate, recomputed every buffer. [PROVEN] The stopwatch is restarted at the end
of each call (:856-857) so the measured interval is roughly the callback period.

- **Cap `num3 * 1200`** = buffers/sec × 1200 s = **1200 seconds = 20 minutes**
  of history, per channel. With the defaults above (`num3 ≈ 10`) that is
  ~12 000 `List<int>` entries. Note the check is `>` and the removal is a
  single `RemoveAt(0)`, so the list can sit at `num3*1200 + 1`.
- The lists are **history only** — there is no separate exponential moving
  average, no min-tracking, and no explicit "noise floor" variable.

Window indices — WaveIn.cs:1004-1040:

```csharp
switch (Main.LevelMeters)
{
case 0: num23 = _0024STATIC_..._leftlist1.Count - num3 * 0 - 2;   break;
case 1: num23 = (int)Math.Round((double)_0024STATIC_..._leftlist1.Count - (double)num3 * 0.5) - 2; break;
case 2: num23 = _0024STATIC_..._leftlist1.Count - num3 * 1 - 2;   break;
case 3: num23 = _0024STATIC_..._leftlist1.Count - num3 * 5 - 2;   break;
case 4: num23 = _0024STATIC_..._leftlist1.Count - num3 * 600 - 2; break;
}
switch (Main.LevelIndicators)
{ ... same five expressions into num24 ... }
int num25 = (int)Math.Round((double)_0024STATIC_..._leftlist1.Count - (double)num3 * 0.15) - 2;
```

```c
num23 = Count - num3*W - 2     // W from Main.LevelMeters    (0, 0.5, 1, 5, 600)
num24 = Count - num3*W - 2     // W from Main.LevelIndicators(0, 0.5, 1, 5, 600)
num25 = round(Count - num3*0.15) - 2   // FIXED
```

Multiplier semantics (`num3` is buffers/s, so `num3*W` is W seconds of buffers):

| multiplier | window | selected by |
|---|---|---|
| `0` | **entire 20-minute history** | Meters/Indicators = 0 |
| `0.5` | 0.5 s | 1 |
| `1` | 1 s | 2 |
| `5` | 5 s | 3 |
| `600` | 600 s (10 min) | 4 |
| `0.15` | 150 ms | **always** — the VOX window |

`Main.LevelMeters` / `Main.LevelIndicators` defaults are both `1` (General.cs:7224,
:7228) → 0.5 s for the two on-screen meters. **[PROVEN] neither affects the
VOX gate** — `LeftVOXMeterValue`/`RightVOXMeterValue` are computed solely from
the `num25` (0.15 s) window (:1083, :1201).

Averaging loop, WaveIn.cs:1041-1059 (left; right is :1159-1177):

```csharp
int num26 = _0024STATIC_..._leftlist1.Count - 1;
for (int i = 0; i <= num26; i++)
{
    if (i > num23) { num15 += (double)_0024STATIC_..._leftlist1[i]; num16++; }   // Meters
    if (i > num24) { num17 += (double)_0024STATIC_..._leftlist1[i]; num18++; }   // Indicators
    if (i > num25) { num19 += (double)_0024STATIC_..._leftlist1[i]; num20++; }   // VOX
}
```

Because the test is `i > index` and `index = Count - num3*W - 2`, the window
spans indices `Count-num3*W-1 … Count-1`, i.e. **`num3*W + 1` items ≈ W seconds
plus one buffer**. The `-2` widens the window by one item; it does not exclude
the newest samples. Early on (`Count` small) the index goes negative and the
whole list is averaged.

Meter arithmetic, WaveIn.cs:1060-1082 (the two on-screen numbers):

```csharp
if (num16 > 0)
{
    num27 = (int)Math.Round(num15 / (double)num16);
    num27 = ((num27 <= 0) ? (-90) : ((int)Math.Round(20.0 * Math.Log10(32767.0 / (double)num27) * -1.0)));
    if (num27 > 0) { num27 = 0; }
    num27 += 90;
}
if (num18 > 0)
{
    num = (int)Math.Round(num17 / (double)num18);
    num = ((num <= 0) ? (-90) : ((int)Math.Round(20.0 * Math.Log10(32767.0 / (double)num) * -1.0)));
    if (num < -90) { num = -90; }
    if (num > 0)   { num = 0; }
}
```

Final VOX values, WaveIn.cs:1083-1099 (left) and :1201-1217 (right):

```csharp
if (num20 > 0)
{
    LeftVOXMeterValue = (int)Math.Round(num19 / (double)num20);
    if (LeftVOXMeterValue > 0)
    {
        LeftVOXMeterValue = (int)Math.Round(20.0 * Math.Log10(32767.0 / (double)LeftVOXMeterValue) * -1.0);
    }
    else
    {
        LeftVOXMeterValue = -90;
    }
    if (LeftVOXMeterValue > 0)
    {
        LeftVOXMeterValue = 0;
    }
    LeftVOXMeterValue += 90;
}
```

```c
avg = mean( list[i] for i in (num25, Count-1] )           // linear, post-√2
db  = (avg > 0) ? round( 20.0 * log10(32767.0 / avg) * -1.0 ) : -90   // == 20*log10(avg/32767)
db  = min(db, 0)
VOXMeterValue = db + 90                                    // integer 0..90
```

**Units** [PROVEN]: `db` is **dBFS** — negative dB relative to full scale
32767, floor **-90 dBFS**, ceiling 0 dBFS. The value *handed to VOX* is
`dB + 90`, i.e. **not dB but a 0…90 meter scale** where `90 == 0 dBFS` and
`0 == ≤ -90 dBFS`. Conversion: `threshold_dBFS = ThreasholdLevel - 90`.

The two other numbers reported to the GUI (`num27`, `num`) keep the 0…90 / -90…0
forms and are pushed on a 50 ms throttle at WaveIn.cs:1219-1237
(`UpdateGUIEvent?.Invoke(121, new int[5]{ _Type, num27, num29, num, num2 })`),
which FrmMain renders as `Label415.Text = <num> + " dB"` (FrmMain.cs ~35700).

## 3. THRESHOLD & GATE (`RecorderVOX.cs`)

```csharp
private int _ThreasholdLevel;                                  // :10
public int ThreasholdLevel { set { _ThreasholdLevel = value; } }  // :28-34   (set-only)
```

```csharp
if (value >= _ThreasholdLevel)      // :94
{
    _ThreasholdTrigger = true;      // :96
}
else
{
    _ThreasholdTrigger = false;     // :100
}
if (_0024STATIC_0024ThreasholdProcess_002420118_0024Temp != _ThreasholdTrigger)
{
    Main.fMain.UpdateGUI(45, null);     // :104  → repaint VOXPictureBox1
}
_0024STATIC_0024ThreasholdProcess_002420118_0024Temp = _ThreasholdTrigger;   // :106
```

`value` is `Math.Max(LeftVOXMeterValue, RightVOXMeterValue)` (WaveIn.cs:539/543)
→ range **0…90**. Comparison is `>=`, inclusive.

Range / default [PROVEN]:
- Clamped to 0…90 on load — FrmMain.cs:14470-14488 (`> 90 → 90`, `< 0 → 0`), same for UID at :14504-14522.
- Slider `VOXTrackBar1.Maximum = 90`, `.Value = 0` (FrmMain.cs:10597, :10601); `VOXTrackBar2.Maximum = 90` (:10685).
- **Default = 30** — `Main.VOXThreashold = 30` (General.cs:6888) and `Main.UIDVOXThreashold = 30` (General.cs:7955). 30 → **-60 dBFS**.
- Config `[VOX THRESHOLD]` → `Main.VOXThreashold = Conversions.ToInteger(text2)` (General.cs:10691); a config with no entry leaves the previous/default value.

50 ms throttle, RecorderVOX.cs:43-90:

```csharp
public void InputAudio(int Value)
{
    if (!Main.ProgramFinishedLoading) { return; }               // :48-51
    ... lazy `Static Stopwatch` init (:52-77) ...
    if (!_0024STATIC_0024InputAudio_002420118_0024sw.IsRunning) { ...Start(); }   // :78-81
    if (_0024STATIC_0024InputAudio_002420118_0024sw.ElapsedMilliseconds > 50)     // :82
    {
        ThreasholdProcess(Value);        // :84  compare + set _ThreasholdTrigger
        Main.fMain.UpdateGUI(130, Value);// :85  → RecordAudioMeter1.Value (FrmMain.cs:35940)
        VOXProcess();                    // :86  latch / hang
        _0024STATIC_0024InputAudio_002420118_0024sw.Reset();   // :87
        _0024STATIC_0024InputAudio_002420118_0024sw.Start();   // :88
    }
}
```

Per throttled tick: exactly one compare against the **instantaneous** value,
one GUI meter push, one latch/hang evaluation, then a full `Reset()+Start()` —
so the clock restarts from the tick, not a modulo, and the effective period is
"≥ 50 ms" (one tick per arriving buffer that lands past the 50 ms mark).
[PROVEN] Values arriving between ticks are **dropped, not aggregated** — no
accumulation, no peak-hold. With 0.1 s buffers the throttle does nothing (every
buffer already exceeds 50 ms); it only bites for shorter buffers.

Same code, separate instance, in `RecorderVOXUID.cs`: compare at :94, throttle
:78-89 (pushing `UpdateGUI(129, Value)` → `RecordAudioMeter2`), own
`Main.UIDVOX_IsVOXLatched`.

## 4. ATTACK / DECAY / HANG

**ATTACK: NOT IMPLEMENTED** as N consecutive above-threshold samples. Search
terms tried: `attack`, `consecutive`, `hysteresis`, `hold`, `debounce`,
`_ThreasholdTrigger`. No counter, no hysteresis, no N-of-M, no ramp exists in
`RecorderVOX.cs` / `RecorderVOXUID.cs` (both files read in full, 161 / 157 lines).
`ThreasholdProcess` (RecorderVOX.cs:92-107) assigns `_ThreasholdTrigger` as a
pure function of the current value: **attack = 0 ticks** — one sample at or
above threshold latches immediately.

**DECAY/HANG: fixed 2 s wall-clock hold, no ramp.** RecorderVOX.cs:109-125:

```csharp
private void VOXProcess()
{
    if (_ThreasholdTrigger)
    {
        _0024STATIC_0024VOXProcess_00242001_0024starttime = DateAndTime.get_Now();   // :113
        if (!Main.VOX_IsVOXLatched)
        {
            Main.VOX_IsVOXLatched = true;      // :116
            LatchStateChanged(8);              // :117
        }
    }
    if (Main.VOX_IsVOXLatched && (double)DateAndTime.DateDiff((DateInterval)9, _0024STATIC_0024VOXProcess_00242001_0024starttime, DateAndTime.get_Now(), (FirstDayOfWeek)1, (FirstWeekOfYear)1) >= 2.0)
    {
        Main.VOX_IsVOXLatched = false;         // :122
        LatchStateChanged(9);                  // :123
    }
}
```

```c
if (triggered) starttime = now;              // refreshed on EVERY above-threshold tick
if (latched && DateDiff(Second, starttime, now) >= 2.0) unlatch();
```

So the hold is **2 s measured from the last above-threshold tick**. `DateDiff`
truncates to whole seconds [INFERENCE on the enum: `(DateInterval)9` = `Second`,
from Microsoft.VisualBasic.DateInterval = Year 0, Quarter 1, Month 2, DayOfYear 3,
Day 4, WeekOfYear 5, Weekday 6, Hour 7, Minute 8, Second 9; corroborated in-tree
by General.cs:37742 which uses `(DateInterval)7` with a `<= 48` bound on an
event-log timestamp — an hour-scale quantity]. With truncation and 50 ms ticks
the true hang is **2.000–2.050 s**. `LatchStateChanged` also fires only on the
transition, so exactly one latch edge and one unlatch edge.

`Reset()` (RecorderVOX.cs:136-140) clears `VOX_IsVOXLatched` and
`_ThreasholdTrigger`; called from Recorder.cs:1164-1166 (max-time timeout) and
FrmMain.cs:32720-32738, :32941-33015, :38689-38691 (stop/restart paths).

[PROVEN] There is no separate "decay" — `_ThreasholdTrigger` goes false the
instant a tick falls below threshold, but the *latched* state (which actually
gates recording) survives 2 s. Also: `_ThreasholdTrigger` is per-instance;
`starttime`, the 50 ms stopwatch, and the `..._0024Temp` edge-detect flag are
VB `Static` locals (fields named `_0024STATIC_...`, RecorderVOX.cs:16-26) and
are therefore shared across instances of the same class [INFERENCE — see the
note in §5].

## 5. WIRING

Construction — FrmMain.cs:14406, :14410 (fields Main.cs:787, :789):

```csharp
Main.RecorderVOX1 = new RecorderVOX();          // FrmMain.cs:14406
Main.RecorderUIDVOX1 = new RecorderVOXUID();    // FrmMain.cs:14410
```

Threshold assignment at startup — FrmMain.cs:14492-14500 (UID at :14526-14534):

```csharp
VOXTrackBar1.Value = Main.VOXThreashold;
((Label)Label174).set_Text(VOXTrackBar1.Value.ToString());
Main.RecorderVOX1.ThreasholdLevel = Main.VOXThreashold;
```

On user change — FrmMain.cs:36903-36911:

```csharp
private void VOXTrackBar1_ValueChanged(object sender, EventArgs e)
{
    if (Main.RecorderVOX1 != null) { Main.RecorderVOX1.ThreasholdLevel = VOXTrackBar1.Value; }
    ((Label)Label174).set_Text(VOXTrackBar1.Value.ToString());
    Main.VOXThreashold = VOXTrackBar1.Value;
}
```

Fed from audio — WaveIn.cs:531-544, inside `ProcessAudio1`'s `switch (_Type)`,
`case 3:` (the Recorder capture stream, `_Type == 3` set at FrmMain.cs:40208):

```csharp
case 3:
{
    if (Main.SOIPClientSomething) { break; }          // :533  (see gotcha below)
    if (Main.RecorderVOX1 != null)
    {
        Main.RecorderVOX1.InputAudio(Math.Max(LeftVOXMeterValue, RightVOXMeterValue));      // :539
    }
    if (Main.RecorderUIDVOX1 != null)
    {
        Main.RecorderUIDVOX1.InputAudio(Math.Max(LeftVOXMeterValue, RightVOXMeterValue));   // :543
    }
```

The same two calls also exist on the playback path at PlaybackProcesser.cs:198-204.

Recorder-mode selection — `RecorderControlsChanged`, FrmMain.cs:37047-37082:

```csharp
if (!((CheckBox)RecorderCheckBox101).get_Checked())      { Main.RecorderMode = 0; }   // :37053-37056
else if (...)
{
    if (((RadioButton)RadioButton1).get_Checked() || ((RadioButton)RadioButton5).get_Checked())
    {
        if (!((CheckBox)CheckBox18).get_Checked())       { Main.RecorderMode = 1; }   // :37072
        else if (((CheckBox)CheckBox18).get_Checked())   { Main.RecorderMode = 2; }   // :37076
    }
    else if (((RadioButton)RadioButton2).get_Checked())  { Main.RecorderMode = 3; ... } // :37081
}
```

| `RecorderMode` | UI | meaning |
|---|---|---|
| 0 | Recorder off | off |
| 1 | `RadioButton1`/`RadioButton5` = "Activity" (FrmMain.resx) | scanner-activity driven |
| 2 | "Activity" + `CheckBox18` = **"Skip Silence"** (FrmMain.resx) | activity driven, silence skipped using `RecorderVOX1` |
| 3 | `RadioButton2` = **"VOX"** (FrmMain.resx) | VOX only; uses `RecorderUIDVOX1` thresholds |

**Which takes precedence** [PROVEN]:
- In mode **1** and **2** the recording is started by the **serial scanner
  activity** path, not by VOX: `ControlRecorder` calls
  `Recorder1.StartRecordLogging(1, recordstarttime)` (FrmMain.cs:25880) after
  matching FreqTGID/Tone; it returns early unless `RecorderMode == 1 || 2`
  (FrmMain.cs:25844). `Main.RadioResponding` is the activity signal
  (Recorder.cs:584 — activity lost clears the record style).
- In mode **2**, VOX does **not** start or stop the recording. Its
  `LatchStateChanged` forwards `UpdateGUI(100, from)` **only when
  `Main.RecorderMode == 3`** (RecorderVOX.cs:127-134):

```csharp
private void LatchStateChanged(int from)
{
    Main.fMain.UpdateGUI(46, null);                  // :129  → repaint VOXPictureBox2
    if (Main.RecorderMode == 3)
    {
        Main.fMain.UpdateGUI(100, from);             // :132
    }
}
```

  In mode 2 the latch instead gates **audio written to the file** and the record
  row's style:
  - FileRecording.cs:308-327 — `case 2:` writes PCM only `if (Main.VOX_IsVOXLatched)`;
    `case 1` and `case 3` write unconditionally.
  - Recorder.cs:592 (`mode == 1 || (mode == 2 && VOX_IsVOXLatched)` → `Style = "Record"`)
    versus Recorder.cs:646-653 (`mode == 2 && !VOX_IsVOXLatched` → `Style = "Record Blank"`).
- In mode **3** ("VOX") the latch *is* the trigger: FrmMain.cs:35666-35692
  (`case 100`) does `Recorder1.StartRecordLogging(2, DateAndTime.get_Now())`
  while latched and `Recorder1.StopRecordLogging(Data)` on the unlatch edge;
  recordings land in `RecordingFolder + "VOX\\"` (Recorder.cs:175-178) and
  pre-record is disabled (`Main.RecorderMode != 3`, Recorder.cs:217).
- The UID twin has the analogous gate: `FileRecordingUID.cs:317-323` writes only
  `if (Main.UIDVOX_IsVOXLatched)`, and `RecorderVOXUID.LatchStateChanged`
  (RecorderVOXUID.cs:127-130) only does `UpdateGUI(44, null)` — it has **no**
  `RecorderMode == 3` forward.

So for a model with no serial activity, mode 3 is the pattern to copy: latch on
`max(L,R) >= threshold`, 2 s hold after the last above-threshold tick, record
only while latched.

**Gotcha** [PROVEN code, INFERENCE on intent]: the guard at WaveIn.cs:533 is
`Main.SOIPClientSomething`, and the sibling cases guard on their own stream
(`:411 !SourceClientSomething`, `:472 !WebServerSomething`), while `:345` shows
the SOIP *server* flag for `_Type == 4` as `SOIPServerSomething`. `_Type == 3`
is the Recorder stream (FrmMain.cs:40208, and left/right levels read from
`LeftLineInLevelREC`/`RightLineInLevelREC` at WaveIn.cs:878-881). Reading the
code literally: **VOX input is dead whenever a Scanner Over IP *client* is
connected.** This looks like a copy/paste of case 4's flag, but I am reporting
the code as written.

**Static-ness caveat** [INFERENCE, high confidence]: ILSpy emitted **zero**
`static` keywords in WaveIn.cs (`grep -c '\bstatic\b' WaveIn.cs` = 0) and zero
`private static` in FrmMain.cs, while emitting `public static RecorderVOX
RecorderVOX1;` normally in Main.cs:787. The fields are named
`_0024STATIC_<Method>_<id>_0024<name>` with companion `StaticLocalInitFlag` +
`Interlocked.CompareExchange` lazy init (WaveIn.cs:748-771) — that is exactly
VB.NET's codegen for a method-level `Static` local, which compiles to a
**Shared (static) field on the type**. So `leftlist1`/`rightlist1`, `num3`'s
counters, and the 50 ms stopwatches are almost certainly **shared across all
four `WaveIn` instances** (one `class WaveIn`, instances created for Type
1/2/3/4 at FrmMain.cs:40041-40305) and across `RecorderVOX` instances. I could
not confirm from IL: no original `ProScan.exe` is in the tree and no
`monodis`/`ikdasm`/`dnfile` is available. **If you mirror this design, make the
history ring per-stream explicitly** — with shared state, four live streams
interleave into one ring and corrupt `num3` and the 20-minute window.

## 6. CALLER / USER SETTINGS

UI controls:
- **`VOXTrackBar1`** — `MyTrackBarAudioVerticle` (FrmMain.cs:3729-3747, :9668),
  `Maximum = 90` (FrmMain.cs:10597), initial `Value = 0` (:10601), added to
  **`TabPage13`** (:10391). Its numeric value is echoed in `Label174`
  (FrmMain.cs:36909; `Label174.Text = "0"` in FrmMain.resx) whose caption in the
  resx is `Label38.Text = "Skip Silence Threshold"` (FrmMain.resx; also set
  literally at FrmMain.cs:37087). Handler `VOXTrackBar1_ValueChanged`
  (FrmMain.cs:36903-36911). Enabled/disabled with `CheckBox18` ("Skip Silence")
  at FrmMain.cs:37083-37095.
- **`VOXTrackBar2`** — same trackbar class, `Maximum = 90` (FrmMain.cs:10685),
  added to **`TabPage6`** (:10611), value echoed in `Label186`
  (FrmMain.cs:36929). Handler `VOXTrackBar2_ValueChanged` (:36923-36931).
- Visual latch indicators: `VOXPictureBox1` (threshold triggered, lime/dark
  green — FrmMain.cs:36943-36963), `VOXPictureBox2` (latched, red/maroon —
  :36965-36985), plus `VOXPictureBox3`/`4` for the UID twin (:36987-37029).

Config keys — General.cs:10677-10691 and :14523-14548:

```csharp
if (Operators.CompareString(text, "VOX THRESHOLD", true) == 0)
{
    if (Common_PS_RF.IsNumericStr(text2))
    {
        Main.VOXThreashold = Conversions.ToInteger(text2);
    }
}
```

Written to the config file at General.cs:25016
(`"[VOX THRESHOLD]=" + Conversions.ToString(Main.VOXThreashold)`) and
General.cs:27143 (`"[UID VOX THRESHOLD]="`). Defaults on a fresh config:
General.cs:6888 (`Main.VOXThreashold = 30`), General.cs:7955
(`Main.UIDVOXThreashold = 30`). The file also exports `[SKIP BLANKS]`
(= `CheckBox18` state, General.cs:25012) and `[ACTIVITY]` (1/2/3 for
RadioButton1/5/2, General.cs:24991-25002) — i.e. the VOX threshold is stored
next to the mode selection, and the threshold alone does nothing unless
"Skip Silence" and/or the VOX radio button is chosen.

---

## Porting notes (the algorithm to copy)

```c
// per capture buffer, per channel  (item = peak-equivalent RMS of THIS buffer)
ms   = sum(x*x) / n            // over the buffer's samples
item = round(sqrt(ms * 2))

ring[ch].push(item);  if (ring[ch].size > buffersPerSec * 1200) ring[ch].pop_front();
// buffersPerSec = round(1000 / avg_ms_between_buffers), cumulative average

// per 50 ms tick  (drop everything in between)
avg = mean( ring[ch] over the last (round(buffersPerSec*0.15) + 1) items )
db  = (avg > 0) ? round(20*log10(32767.0/avg) * -1.0) : -90
db  = min(db, 0)
latchValue = db + 90                  // 0..90 ; 90 == 0 dBFS

if (max(latchValueL, latchValueR) >= threshold) { lastHit = now; latched = true;  }
if (latched && elapsedSeconds(lastHit) >= 2) latched = false;
```

- Default threshold 30 → **-60 dBFS**; range 0…90 (0 → -90 dBFS, 90 → 0 dBFS).
- Attack 0 buffers; hang 2.0 s (± one tick) after the last above-threshold tick.
- Effective detection window ≈ `0.15 s × buffersPerSec + 1 buffer` ≈ 300 ms with
  0.1 s buffers — quantised to whole buffers, so use short buffers (e.g. 10–20 ms)
  if you want the 150 ms window to mean anything.
- No noise-floor estimation, no AGC, no hysteresis, no serial/N-of-M attack, no
  frequency weighting: the gate is a plain per-channel smoothed RMS in dBFS with
  a 2 s hold. "Background noise floor" in ProScan is implicit — a *fixed*
  user threshold on that dBFS value, not an adaptive floor estimate.
