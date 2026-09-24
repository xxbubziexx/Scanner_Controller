# ProScan audio input/output pipeline & device wiring

Reverse-engineered from `ProScan.exe` (VB.NET, .NET 4.8, unobfuscated). Every claim carries
its provenance as `file:line` in `decompiled/` (ILSpy 7.2.1 output).

Source files:
- `WaveIn.cs` — the capture engine (custom winmm P/Invoke, 1787 lines)
- `WaveOut.cs` — the playback engine (custom winmm P/Invoke, 305 lines)
- `PlaybackProcesser.cs` — volume scaling, mono/stereo conversion, mixer, meters
- `Native.cs` — every P/Invoke declaration (`winmm.dll`, `lame_enc.dll`)
- `FrmMain.cs:40000-40435` — the four `StartWaveIn*` methods and `PlaybackProcessing`
- `General.cs:32330-32435` — device enumeration and name→ID resolution
- `ConvertPCMToMP3.cs` — LAME wiring and the only resampling in the app
- `FrmOptions.cs`, `AudioControl.cs` — the device/format UI and the volume sliders

---

## 0. The headline answers

| Question | Answer |
|---|---|
| Audio library for **capture**? | **No library.** Hand-rolled `winmm.dll` P/Invoke in `WaveIn.cs`. |
| Audio library for **playback**? | **No library** on the main path. Hand-rolled `winmm.dll` P/Invoke in `WaveOut.cs`. |
| Where is NAudio 1.7.3 used? | Only **3 places**: `PlayMP3_C` (`NAudio_1_7_3.Wave.WaveOut`), `PlayMP3_D` (`DirectSoundOut`), `ConvertMP3ToPCM` (ACM/DMO MP3 decode). |
| Is WASAPI used? | **No.** `WasapiOut`/`WasapiCapture` exist in the bundled NAudio but are never instantiated by ProScan. |
| Latency mechanism | Raw `WAVEHDR` buffer rings, **no `BufferMilliseconds`/`NumberOfBuffers`**, no `BufferedWaveProvider`. |
| Capture ring | **50 buffers**, each sized `round(SampleRate × 0.2)` samples |
| Output ring | **25 buffers** |
| Format | PCM `wFormatTag=1`, **16-bit, 2 channels (stereo), always** on both capture and playback |
| Resampling | Only via **LAME's `dwReSampleRate`**, and only on the network-audio path |
| Software squelch? | **None.** Squelch is the scanner's job; VOX is the only audio-level gate. |
| Dual-scanner stereo panning? | **Does not exist.** ProScan's answer is two instances in two folders. |

If you take one thing from this document for your dual-scanner app: **ProScan has no
dual-scanner feature.** Details in §9 — that assumption in your brief is worth dropping before
you design around it.

---

## 1. There is no audio library

`Native.cs:719-740` declares the whole audio surface by hand:

```csharp
[DllImport("winmm.dll")] static extern int waveInGetNumDevs();
[DllImport("winmm.dll", EntryPoint = "waveInGetDevCapsA")] static extern int waveInGetDevCaps(int uDeviceID, ref WAVEINCAPS lpCaps, int uSize);
[DllImport("winmm.dll")] static extern int waveOutGetNumDevs();
[DllImport("winmm.dll", EntryPoint = "waveOutGetDevCapsA")] static extern int waveOutGetDevCaps(int uDeviceID, ref WAVEOUTCAPS lpCaps, int uSize);
```

Plus `waveInOpen` / `waveInPrepareHeader` / `waveInAddBuffer` / `waveInStart` /
`waveInUnprepareHeader`, `waveOutOpen` / `waveOutPrepareHeader` / `waveOutWrite` /
`waveOutReset` / `waveOutClose`, `CreateEvent` / `ResetEvent` / `WaitForSingleObject`, and the
`lame_enc.dll` encoder entry points `beInitStream` / `beEncodeChunk` / `beDeinitStream` /
`beCloseStream` (`Native.cs:730-740`).

That's the whole stack. **Nothing sits between ProScan and winmm** on the live path.

### Where NAudio actually appears

Only three call sites, all on secondary paths:

| File | Line | What it constructs |
|---|---|---|
| `PlayMP3_C.cs` | `:12` | `NAudio_1_7_3.Wave.WaveOut()` as `IWavePlayer` |
| `PlayMP3_D.cs` | `:15` | `new DirectSoundOut()` |
| `ConvertMP3ToPCM.cs` | `:87`, `:92` | `AcmMp3FrameDecompressor` / `DmoMp3FrameDecompressor` |

Plus **`mciSendString`** (Windows Media Control Interface) for MP3 playback in
`PlayMP3_A.cs:20` and throughout `Recorder.cs` (e.g. `:1353`, `:1401`, `:1466`, `:1651`) — i.e.
some playback paths hand the file to the OS codec instead of decoding it themselves.

So **five different playback implementations coexist** (`PlayMP3_A` MCI, `B` ?, `C` NAudio
waveOut, `D` DirectSound, plus the custom `WaveOut` for live audio). If you're picking one
architecture for your own app, note that ProScan is the cautionary tale, not the model.

---

## 2. The four capture channels

ProScan can run **four independent capture streams simultaneously**, each with its own device,
channel mode, sample rate and MP3 bitrate. They are distinguished by a `Type` integer:

| `Type` | Channel | WaveIn field | Device state | Channels state | Rate state | Bitrate state |
|---|---|---|---|---|---|---|
| 1 | **Source Client** (SC) | `WaveInSC` | `WaveInIDSC` | `ChannelsSC` | `SampleRateSC` | `mp3BitRateSC` |
| 2 | **Web Server** (WS) | `WaveInWS` | `WaveInIDWS` | `ChannelsWS` | `SampleRateWS` | `mp3BitRateWS` |
| 3 | **Recorder** (REC) | `WaveInRec` | `WaveInIDREC` | `ChannelsRecord` | `SampleRateREC` | `MP3BitRateREC` |
| 4 | **Scanner Over IP server** (SOIP) | `WaveInSOIP` | `WaveInIDSOIP` | `ChannelsSOIP_S` | `SampleRateSOIP_S` | `mp3BitRateSOIP` |

Started by `StartWaveInSC` (`FrmMain.cs:40000`), `StartWaveInWS` (`:40076`), `StartWaveInSOIP`
(`:40260`), and the recorder's equivalent — all four follow the same shape:

```csharp
// FrmMain.cs:40021-40046 (SC), the others are identical modulo names
if (Main.WaveInIDSC != "None")
{
    if (Main.WaveInIDSC == "") Main.WaveInIDSC = "Primary Sound Capture Device";
    int num = General.GetWaveInDeviceID(Main.WaveInIDSC);
    if (num == 9999) { Main.WaveInIDSC = "Primary Sound Capture Device"; num = -1; }
    if (num == -2)   // network audio
    {
        if (URLAudioClient1 == null) URLAudioClient1 = new URLAudioClient(Main.URLAudioList[0]);
        WaveInSC = new WaveIn(1, num, "Mono", 8000, Main.mp3BitRateSC, Main.SampleRateSC, 0, Main.URLAudioBytes, ref WaveErrorDesc);
    }
    else             // local sound card
    {
        WaveInSC = new WaveIn(1, num, Main.ChannelsSC, Main.SampleRateSC, Main.mp3BitRateSC, 0, 50,
                              Math.Round(Main.SampleRateSC * 0.2), ref WaveErrorDesc);
    }
}
```

Each `StartWaveIn*` also has a **no-op guard**: if `ForceRestart` is false and the cached
device/channels/rate/bitrate are unchanged, it returns immediately (`FrmMain.cs:40007-40010`).
So changing any of those four values is what triggers a device re-open.

On failure the WaveIn is disposed, controls are cleared, and the user gets
*"Unable To Open Source Client Input Sound Device — The input Is Not enabled Or Not plugged
In. Windows Message: &lt;err&gt;"* (`FrmMain.cs:40054`).

---

## 3. Capture engine — `WaveIn` (`WaveIn.cs`)

### Constructor (`WaveIn.cs:174`)

```csharp
public WaveIn(int Type, int DeviceID, string ChannelMode, int SamplesPerSec,
              int mp3BitRate, int ResampleRate, int Buffers, int BufferSize, ref string WaveErrorDesc)
```

### Device IDs (`General.cs:32367`)

| Value | Meaning |
|---|---|
| `-1` | `"Primary Sound Capture Device"` → WAVE_MAPPER (system default) |
| `-2` | `"URL"` → **no wave device**; network audio via `URLAudioClient` |
| `0..N-1` | index into `waveInGetDevCaps()` order |
| `9999` | not found → caller falls back to `-1` |

### Device list (`General.cs:32330`)

```csharp
if (_RadioType == BCD536HP || _RadioType == SDS200 || _RadioType == SDS200E)
{
    array = new string[waveInGetNumDevs() + 3];
    array[0] = "Primary Sound Capture Device";
    // array[1..N] = waveInGetDevCaps(i).szPname.Trim()
    array[array.Length - 2] = "URL";
    array[array.Length - 1] = "None";
}
else
{
    array = new string[waveInGetNumDevs() + 2];
    array[0] = "Primary Sound Capture Device";
    array[array.Length - 1] = "None";
}
```

**`"URL"` is offered as an audio input only for the same three network-capable models** that get
it in the COM-port list (`BCD536HP`, `SDS200`, `SDS200E`) — consistent with
`COMPORT-AND-SCANNER-SELECTION.md` §3. Everything else is serial-plus-sound-card only.

Playback list (`General.cs:32395`) is simpler: `["Primary Sound Playback Device", <names…>, "None"]`,
and `GetWaveOutDeviceID` (`:32414`) maps `-1` / `0..N-1` / `9999` the same way. There is no
`-2` on the output side.

### The format — always 16-bit stereo PCM (`WaveIn.cs:198-204`)

```csharp
waveFormat.wFormatTag      = 1;      // WAVE_FORMAT_PCM
waveFormat.nChannels       = 2;      // STEREO, hardcoded
waveFormat.nSamplesPerSec  = _SamplesPerSec;
waveFormat.wBitsPerSample  = 16;
waveFormat.nBlockAlign     = (short)Math.Round(nChannels * (wBitsPerSample / 8.0));  // = 4
waveFormat.nAvgBytesPerSec = nSamplesPerSec * nBlockAlign;
```

**`_ChannelMode` ("Mono"/"Stereo") never reaches the driver.** Capture is *always* stereo; the
Mono setting selects which *software-processed buffer* is used downstream (§6). That's the
first thing to know if you're copying this: the device is opened stereo regardless, and mono is
a downmix done in managed code.

### Open flags and callback model (`WaveIn.cs:209-233`)

```csharp
_lEventHandle = Native.CreateEvent(IntPtr.Zero, bManualReset: false, bInitialState: false, ref lpName);
Native.ResetEvent(_lEventHandle);
int num = 1;
do {
    _Result = Native.waveInOpen(ref _WavePtr, _DeviceID, ref _WaveFormat, _lEventHandle, 0, 327680);
    if (_Result == 0) break;
    if (num > 4) { WaveErrorDesc = Native.GetErrorDesc(_Result); return; }
    num++;
} while (num <= 5);
```

- `327680` = `0x50000` = **`CALLBACK_EVENT`**. So it's event-driven, not a function callback —
  the driver signals a Win32 event and a managed thread drains it.
- **The open is retried up to 5 times.** No delay between attempts; a transient failure just
  spins.

### Buffer allocation (`WaveIn.cs:234-250`)

```csharp
_WaveHeader = new Native.WAVEHDR[_Buffers];
_HeaderHandle = GCHandle.Alloc(_WaveHeader, GCHandleType.Pinned);
for (int i = 0; i < _Buffers; i++)
{
    _WaveHeader[i].lpData         = Marshal.AllocHGlobal(_WaveFormat.nAvgBytesPerSec * 2);  // 2 SECONDS
    _WaveHeader[i].dwUser         = new IntPtr(i);
    _WaveHeader[i].dwBufferLength = _BufferSize * 2;                                        // used length
    _WaveHeader[i].dwBytesRecorded = 0; _WaveHeader[i].dwFlags = 0;
    _WaveHeader[i].dwLoops = 0; _WaveHeader[i].lpNext = IntPtr.Zero; _WaveHeader[i].reserved = 0;
    Native.waveInPrepareHeader(_WavePtr, ref _WaveHeader[i], Marshal.SizeOf(_WaveHeader[i]));
    Native.waveInAddBuffer(_WavePtr, ref _WaveHeader[i], Marshal.SizeOf(_WaveHeader[i]));
}
Native.waveInStart(_WavePtr);
```

**The allocation (`nAvgBytesPerSec × 2` = 2 seconds) is far larger than the used length
(`_BufferSize × 2`).** Not a bug, but it means the ring's real capacity is set by
`dwBufferLength`, not by the `AllocHGlobal` size. If you're sizing your own ring, use the
`dwBufferLength` numbers.

### The drain thread (`WaveIn.cs:273-326`)

```csharp
Thread thread = new Thread(WaveIn); thread.Name = "Audio"; thread.Start();

// WaveIn()
while (true)
{
    if (!_WaveInRunning) break;
    Native.WaitForSingleObject(_lEventHandle, 1000);   // 1 s timeout
    Native.ResetEvent(_lEventHandle);
    for (_CountBuffer = 0; _CountBuffer < _Buffers; _CountBuffer++)
    {
        while ((_WaveHeader[_CountBuffer].dwFlags & 1) != 1)   // WHDR_DONE = 0x1
        {
            Thread.Sleep(1);                                   // busy-wait, 1 ms
            if (!_WaveInRunning) return;
        }
        if (!_WaveInRunning || Main.ProgramClosing) return;
        WaveInPointer(_WaveHeader[_CountBuffer].lpData);
        _Result = Native.waveInAddBuffer(_WavePtr, ref _WaveHeader[_CountBuffer], ...);
    }
}
```

Two things worth noting for your own design:

- **`WHDR_DONE` is polled with `Thread.Sleep(1)`**, not waited on. With 50 buffers, each
  iteration walks all 50 and can spin up to 1 ms per not-yet-done buffer. That's a
  busy-wait inside the realtime audio thread — cheap per buffer but not free, and it means the
  thread can be blocked while the driver wants buffers back.
- **The event is drained, then every buffer is processed in order.** Buffers are not
  individually dispatched; the whole array is swept per wakeup.

### Per-buffer DSP chain (`WaveIn.cs:328-402`)

```csharp
public void WaveInPointer(IntPtr Ptr)
{
    Marshal.Copy(Ptr, _BufferStereo, 0, _BufferStereo.Length);   // raw interleaved stereo
    _Notch1 = new NotchFilter(2, _SamplesPerSec, Main.FiltersList);
    if (_Notch1 != null) _Notch1.ProcessEq(ref _BufferStereo);   // notch/EQ filter
    int LeftVOXMeterValue = 0, RightVOXMeterValue = 0;
    ProcessAudio2(_BufferStereo, _BufferMono, ref LeftVOXMeterValue, ref RightVOXMeterValue);
    ...
}
```

So per buffer: **copy → notch/EQ filter → level/VOX analysis → playback → encode.**

Buffer sizes:
- `_BufferStereo = new short[_BufferSize]` (`WaveIn.cs:194`)
- `_BufferMono = new short[_BufferSize / 2]` (`:197`) — **half, because it's a downmix of
  interleaved stereo**

---

## 4. Buffer sizes and latency — the actual numbers

There is **no `BufferMilliseconds`, no `NumberOfBuffers`, no `BufferedWaveProvider`, no
`DiscardOnBufferOverflow`** anywhere in ProScan. The latency comes from the raw ring sizes at
the two `WaveIn` construction sites:

| Path | `Buffers` | `BufferSize` | `ResampleRate` |
|---|---|---|---|
| Local sound card | **50** | `round(SampleRate × 0.2)` | **0** |
| Network (`URL`, DeviceID `-2`) | **0** | `Main.URLAudioBytes` | `SampleRate` (the user's rate) |

`FrmMain.cs:40045` (local) and `FrmMain.cs:40041` (URL).

For playback, `PlaybackProcesser.cs:1115`:

```csharp
Main.WaveOut1 = new WaveOut(num, _SamplesPerSec, 25, Type);   // 25 buffers
```

### The arithmetic, and one ambiguity I can't settle from source

`BufferSize = round(SampleRate × 0.2)` reads as "0.2 seconds of samples". At the default
22050 Hz that's **4410**. Then:

```
dwBufferLength = BufferSize × 2 bytes = 8820 bytes
device format  = 16-bit STEREO, nBlockAlign = 4
8820 bytes ÷ 4 bytes/frame     = 2205 stereo frames
2205 ÷ 22050 Hz                = 0.100 seconds
```

**So the effective per-buffer duration is ~0.1 s, not 0.2 s**, because `BufferSize` is computed
as if the samples were mono while the device runs stereo and `dwBufferLength` is
`BufferSize × 2` *bytes*. Either the `× 2` should have been `× 4`, or `BufferSize` was intended
as a per-channel count. **I can't determine intent from source alone** — flagging it rather
than picking a number.

What is not ambiguous:

- **Capture ring depth ≈ 50 × 0.1 s ≈ 5 seconds** at 22050 Hz stereo (if the 0.1 s reading is
  right), or 10 s if `BufferSize` is per-channel.
- **Playback ring = 25 slots.** Each slot is *allocated* `nAvgBytesPerSec × 2` (2 s of stereo:
  176,400 bytes at 22050 Hz) but the actual write sets `dwBufferLength = Buffer.Length × 2`
  (`WaveOut.cs:58`), so used size depends on the incoming buffer.
- **The two rings are wildly oversized** relative to any sane low-latency target. That's
  deliberate for a scanner app (it must not drop audio while the UI is busy), but it means
  ProScan's own latency is dominated by downstream buffering, not by these numbers.

> **The manual's own latency story is about the *network* player**, not the local path:
> *"The audio player network / latency buffer delays the audio… WinAmp desktop version seems to
> have the best latency that's about 3 seconds"* and a changelog entry *"Low latency (1.3
> second) audio player on the Web Server web page"*. Local playback isn't latency-tuned.

---

## 5. Output engine — `WaveOut` (`WaveOut.cs`)

```csharp
public WaveOut(int DeviceID, int SamplesPerSec, int Buffers, int Type)
```

### Device resolution (`PlaybackProcesser.cs:1096-1116`)

```csharp
private void StartWaveOut(int Type)
{
    if (Main.WaveOut1 != null) { Main.WaveOut1.Dispose(); Main.WaveOut1 = null; }
    if (Main.WaveOutID != "None")
    {
        if (Main.WaveOutID == "") Main.WaveOutID = "Primary Sound Playback Device";
        int num = General.GetWaveOutDeviceID(Main.WaveOutID);
        if (num == 9999) { Main.WaveOutID = "Primary Sound Playback Device"; num = -1; }
        Main.WaveOut1 = new WaveOut(num, _SamplesPerSec, 25, Type);
    }
}
```

`"None"` → **no `WaveOut` object is created at all** (silent, playback simply doesn't happen).

### Format (`WaveOut.cs:85-91`) — same as capture

```csharp
waveFormat.wFormatTag      = 1;      // PCM
waveFormat.nChannels       = 2;      // STEREO, hardcoded
waveFormat.nSamplesPerSec  = _SamplesPerSec;
waveFormat.wBitsPerSample  = 16;
```

### Open (`WaveOut.cs:92`)

```csharp
_Result = Native.waveOutOpen(ref _WavePtr, _DeviceID, ref _WaveFormat, IntPtr.Zero, 0, 0);
```

**`dwFlags = 0` = `CALLBACK_NULL`, and `dwCallback = IntPtr.Zero`.** ProScan deliberately takes
no completion callback on playback and never tests `WHDR_DONE` on the output side. See the bug
in §8.

### The write loop (`WaveOut.cs:39-75`)

```csharp
public void WaveOutPlay(short[] Buffer)
{
    if (!_WaveOutRunning || Main.MuteSOIPClientAudio || _WaveHeader == null || _WavePtr == IntPtr.Zero)
        return;
    if (_CountBuffer >= _Buffers) _CountBuffer = 0;                 // ring wrap
    if (_WaveHeader[_CountBuffer].lpData == IntPtr.Zero || !_WaveOutRunning) return;
    Marshal.Copy(Buffer, 0, _WaveHeader[_CountBuffer].lpData, Buffer.Length);
    _WaveHeader[_CountBuffer].dwBufferLength = Buffer.Length * 2;
    _Result = Native.waveOutWrite(_WavePtr, ref _WaveHeader[_CountBuffer], ...);
    _CountBuffer++;
}
```

Note `Marshal.Copy(short[], …)` copies `Buffer.Length` **shorts** (= 2 × Length bytes), then
`dwBufferLength = Buffer.Length * 2` bytes — consistent. ✓

---

## 6. Volume, mute, and the stereo/panning mechanism

**This is where your "panning" question is answered — and it isn't panning, it's independent
per-channel gain that has the same effect.**

### The mixer — `PlaybackProcesser` (`PlaybackProcesser.cs`)

`AudioIn(Type, BufferMonoStereo)` (`:165`) converts mono↔stereo and applies gain, then routes
to the recorder and to `WaveOut`:

```csharp
// :185 / :194
ProcessAudioStereo(BufferMonoStereo, _BufferMono, _BufferStereo, ref LeftVOXMeterValue, ref RightVOXMeterValue);
ProcessAudioMono  (BufferMonoStereo, _BufferMono, _BufferStereo, ref LeftVOXMeterValue, ref RightVOXMeterValue);
...
// :229 / :258   recording
Main.FileRecord1.AudioPCM(_BufferStereo);     // or _BufferMono
...
// :290          playback — ALWAYS the stereo buffer
Main.WaveOut1.WaveOutPlay(_BufferStereo);
```

### Stereo input path (`PlaybackProcesser.cs:400-418`)

```csharp
double level  = ScaleOutVolume(Main.LeftSpeakerOut);
double level2 = ScaleOutVolume(Main.RightSpeakerOut);
for (int i = 0; i <= BufferMonoStereo1.Length - 1; i++)
{
    if (i % 2 == 0)   // even interleaved sample = LEFT
        num5 += Math.Pow(BufferStereo[i] = Common_PS_RF.LimitShort1(BufferMonoStereo1[i], level), 2.0);
    else              // odd = RIGHT
        num7 += Math.Pow(BufferStereo[i] = Common_PS_RF.LimitShort1(BufferMonoStereo1[i], level2), 2.0);
    // mono downmix = average of the pair
    BufferMono[(i - 1) / 2] = (short)Math.Round(BufferStereo[i] / 2.0 + BufferStereo[i - 1] / 2.0);
}
```

### Mono input path (`PlaybackProcesser.cs:801-817`) — the panning equivalent

```csharp
double num4 = ScaleOutVolume(Main.LeftSpeakerOut);
double num5 = ScaleOutVolume(Main.RightSpeakerOut);
double level = Math.Max(num4, num5);
for (int i = 0; i <= BufferMonoStereo1.Length - 1; i++)
{
    BufferMono[i]          = LimitShort1(BufferMonoStereo1[i], level);        // mono copy uses the LOUDER side
    BufferStereo[i * 2]     = LimitShort1(BufferMonoStereo1[i], num4);        // left  = mono × left gain
    BufferStereo[i * 2 + 1] = LimitShort1(BufferMonoStereo1[i], num5);        // right = mono × right gain
}
```

**A mono source is duplicated to both output channels with *different* gains.** Set
`LeftSpeakerOut` high and `RightSpeakerOut` low and the audio moves to the left. That is
balance/pan control, implemented as two independent gain multipliers — and it's the mechanism
you'd reuse to put scanner A on the left and scanner B on the right.

### `LimitShort1` (`Common_PS_RF.cs:254`) — the gain stage

```csharp
public static short LimitShort1(double Input, double Level)
{
    double num = Input * Level;
    if (num < -32768.0) num = -32768.0;
    if (num >  32767.0) num =  32767.0;
    ...
}
```

Multiply, then **hard-clamp** to int16 range (clips rather than wrapping).

### `ScaleOutVolume` (`PlaybackProcesser.cs:1119`) — the exact volume curve

```csharp
private double ScaleOutVolume(int Value)
{
    if (Value < 0)  Value = 0;
    if (Value > 60) Value = 60;
    if (Value == 0) return 0.0;
    return Math.Pow(10.0, ((double)Value - 30.0) / 20.0);
}
```

| Slider | Gain | dB |
|---|---|---|
| 0 | **hard 0** (special-cased true mute) | −∞ |
| 20 | 0.316 | −10 dB |
| **30** | **1.0** (unity) | **0 dB** — default |
| 40 | 3.16 | +10 dB |
| 60 | 31.6 | **+30 dB** |

So it's a **±30 dB range in 1 dB steps, with 30 as unity**, and `0` is a genuine mute rather
than −30 dB. **Slider 60 gives ~31× amplification** — that will hard-clip any real signal, and
`LimitShort1` will clamp it into flat-topped distortion rather than wrapping. Range is clamped
inside the function; the `MAXOUTVOLUMELEVEL = 60` constant at `Main.cs:353` is **never
referenced anywhere** (same dead-constant pattern as `CommType`).

Defaults: `LeftSpeakerOut = 30`, `RightSpeakerOut = 30` (`General.cs:7004`, `:7008`).

UI: `AudioControl.cs:5546-5555` — `TrackBar203` (left) writes `Main.LeftSpeakerOut` and mirrors
into `TrackBar503`; if a link condition is set, `Main.RightSpeakerOut = Main.LeftSpeakerOut` and
`TrackBar204`/`TrackBar504` follow. So there are **paired sliders per channel group** with an
optional lock.

> ### ⚠️ Records are post-volume
> `ProcessAudioStereo`/`ProcessAudioMono` write the **scaled** samples into `_BufferStereo` /
> `_BufferMono` (`PlaybackProcesser.cs:411`, `:816-817`), and *those same buffers* are what get
> handed to the recorder (`:229`, `:258`) and to the MP3 encoder (`:242`, `:271`).
>
> **There is no independent record level.** If you drag the playback volume to a 3× boost, your
> recordings clip with it; drop it to −20 dB and your recordings are quiet. If your app needs
> archives independent of monitoring level, apply gain **after** the record tap — ProScan does
> not, and it's the single most likely thing to surprise you when copying this design.

### Mute flags

| Flag | Where checked | Effect |
|---|---|---|
| `MuteSOIPClientAudio` | `WaveOut.cs:41`, `General.cs:32682` | Blocks playback at the write call |
| `MuteAudioWiFi` | `URLAudioClient.cs:554` | Mutes the LAN/WiFi audio client |
| `MuteAllInstances` | `VirtualScanner.cs:645` | Mutes every running instance |
| `MuteCharacteristics` | `VirtualScanner.cs:613-621` | 0/1/2 selecting **which scanner models** the mute applies to (`BCD536HP`/`UBCD536PT`, `SDS200`/`SDS200E`) |

Muting is **all-or-nothing per stream**. There is no per-transmission fade, no ducking, and no
mute driven by signal level.

Also note the **broadcast-lockout mute**, which zeroes the buffer in place before encoding
(`WaveIn.cs:418-424` and `:479-485`):

```csharp
if (Main.BroadcastLockoutFlagSC) { for (int k = 0; k <= _BufferStereo.Length - 1; k++) _BufferStereo[k] = 0; }
```

Zero-fill rather than skipping the encode, so the MP3 stream keeps a continuous timeline.

---

## 7. Sample rate, format and resampling

### Defaults (`General.cs:6900-7004`)

| Setting | Default |
|---|---|
| `SampleRateSC` / `WS` / `REC` / `SOIP_S` | **22050 Hz** |
| `ChannelsSC` / `WS` / `Record` / `SOIP_S` | **`"Mono"`** |
| `MP3BitRateREC` / `mp3BitRateSC` | **16** (kbps) |
| `WaveInID*` | `"None"` |
| `WaveOutID` | `"Primary Sound Playback Device"` |

The manual fixes the bitrate story: *"It's recommended when streaming to the public, the mode
should be mono"* and *"the MP3 Bitrate is fixed at 16 (mono) / 32 (stereo)"*. The code forces
it too — `General.cs:10843` and `:11312` set the bitrate to `32` when stereo is chosen.

### Resampling is LAME's job, and only on the network path

`ConvertPCMToMP3` (`ConvertPCMToMP3.cs:53`, `:99-116`):

```csharp
public ConvertPCMToMP3(int Type, string Mode, int SampleRate, int mp3BitRate, int ResampleRate)
{
    if (SampleRate == 0) throw new Exception("SampleRate = 0 not supported");
    be_Config_Format.dwConfig         = 256;
    be_Config_Format.dwStructVersion  = 1;
    be_Config_Format.nMode            = (stereo ? 0 : 3);     // 0 = stereo, 3 = mono
    be_Config_Format.dwSampleRate     = SampleRate;           // INPUT rate
    be_Config_Format.dwBitrate        = mp3BitRate;
    be_Config_Format.bNoRes           = 0;                    // resampling permitted
    be_Config_Format.bEnableVBR       = 0;                    // CBR
    if (SampleRate < 32000) { be_Config_Format.dwMpegVersion = 0; }   // MPEG2/2.5
    if (ResampleRate == 0) { /* no resample */ }
    else { be_Config_Format.dwReSampleRate = ResampleRate; }  // OUTPUT rate
    Native.beInitStream(ref _Be_Config_Format, ref _dwSamples, ref _dwBufferSize, ref _HandleStream);
}
```

So:

- **The only resampling in ProScan is `lame_enc.dll`'s `dwReSampleRate`.**
- `ResampleRate = 0` on the local-sound-card path → **no resampling**; LAME encodes at whatever
  the capture device was opened at.
- On the network path `ResampleRate = Main.SampleRateSC` while `SamplesPerSec = 8000`, so
  **8 kHz scanner Ethernet audio is upsampled to the configured rate (default 22050 Hz) by
  LAME.** Upsampling, not downsampling — a little odd, but that's what the config says.
- `nMode = 3` is LAME's mono mode; `0` is stereo. This is where the Mono/Stereo setting finally
  matters to the encoder.
- MPEG version is picked from the input rate (`< 32000` → MPEG2/2.5), which matters for
  bitrate/frame-size limits.

### Input vs output rate matching

**They are not matched — and they don't have to be.** `WaveOut` is constructed with
`_SamplesPerSec`, the *same* value the capture side was opened at for that channel
(`PlaybackProcesser` is created as `new PlaybackProcesser(ChannelMode, SampleRate)` and passes
its `_SamplesPerSec` straight to `WaveOut`). So on the local path the output device is opened at
the capture rate, and there is **no rate conversion between capture and playback at all**.

The mismatch case is the network path: capture runs at a fixed 8000 Hz, and if you listen to it,
`PlaybackProcessing(3, …)` routes it to the same single `WaveOut` with the *channel's* rate.
That's the one place where an unexpected rate can reach the output device.

Also relevant: `PlayPCM`/`WaveIn` MP3 playback goes through **`ConvertMP3ToPCM`** which decodes
via `AcmMp3FrameDecompressor` or `DmoMp3FrameDecompressor`, constructing an `Mp3WaveFormat`
from the actual frame header (`ConvertMP3ToPCM.cs:87`, `:92`) — i.e. the file's own rate, so
playback of a recorded file is at the file's rate, not the live rate.

---

## 8. Squelch — there isn't any (the important negative)

I searched every file for `squelch` case-insensitively. **All 20+ hits are database/data-model
fields, not audio-path logic:**

- `DynamicDatabase_B.cs:12438` etc. — a combo box offering `"10 Sec.", "30 Sec.", "Squelch",
  "Keypress", "Infinite"` as a **Channel Delay** setting
- `DynamicDatabase_D.cs:17176` — `"Always Off", "Always On", "Squelch", "Keypress", "Key +
  Squelch"` as a **scanner data setting**
- `DynamicDatabase_B.cs:18835` etc. — `.Replace("Squelch", "SQ")` when writing the scanner's
  memory format over the serial link

**Conclusion: ProScan applies no software squelch to the audio.** The audio pipeline is
continuously open and flowing from device to encoder/playback. Squelch is the *scanner's*
hardware function, and ProScan only reads the resulting state (`ScannerData1.Activity`) over the
control port to decide when to start/stop **recording** — not whether to pass audio.

### VOX is the only audio-level gate

`WaveIn.cs:392`, `:537-543`:

```csharp
ProcessAudio2(_BufferStereo, _BufferMono, ref LeftVOXMeterValue, ref RightVOXMeterValue);
...
if (Main.RecorderVOX1 != null)    Main.RecorderVOX1.InputAudio(Math.Max(LeftVOXMeterValue, RightVOXMeterValue));
if (Main.RecorderUIDVOX1 != null) Main.RecorderUIDVOX1.InputAudio(Math.Max(LeftVOXMeterValue, RightVOXMeterValue));
```

And `ProcessAudio2` (`WaveIn.cs:670`) maintains per-channel energy histories
(`leftlist1` / `rightlist1`, capped at `num3 × 1200` entries, `:995`), computes averages over
several window lengths (`:1007-1041`, multiples of `num3` up to `num3 × 600`), and converts to
dB (`:1085-1094`):

```csharp
LeftVOXMeterValue = (int)Math.Round(num19 / (double)num20);
if (LeftVOXMeterValue > 0)
    LeftVOXMeterValue = (int)Math.Round(20.0 * Math.Log10(32767.0 / LeftVOXMeterValue) * -1.0);
else
    LeftVOXMeterValue = -90;
```

Note the **`-90` dB floor** — the same floor the meters use in `ClearControlsSC`
(`FrmMain.cs:40063`: `AudioMeter401.Value = -90`).

**VOX takes `Math.Max(left, right)`** — the louder channel wins. So even the level detector
collapses the stereo pair rather than treating the channels separately. There is no
left-scanner / right-scanner split anywhere in the audio path.

The manual confirms the design: *"VOX — An alternative recording mode that's not dependent on
the scanner activity but depends on the audio level above the [threshold]"*.

---

## 9. "Dual scanner" — ProScan does not have it

Your brief mentions "listening to dual scanners (Scanner A / Scanner B)". **No such feature
exists in ProScan**, and I want to be explicit rather than let you design around a fiction:

- No `ScannerA`/`ScannerB` state, no dual-scanner form, no A/B routing.
- The grep for `dual|two scanner|scanner a|scanner b|second scanner` across the manual returns
  only unrelated matches (baud rates, channel lists).
- **There is exactly one `PlaybackProcesser1` and one `WaveOut1`** (`FrmMain.cs:40426`,
  `PlaybackProcesser.cs:1115`). One output stream, arbitrated by the `Type` parameter — not
  mixed, not panned per scanner.

What ProScan actually offers instead:

1. **Two instances in two folders.** The manual is explicit (*"Each instance of ProScan must be
   in its own folder for the options to be unique for each instance"*), and `MuteAllInstances`
   (`VirtualScanner.cs:645`) exists specifically to mute one instance while listening to the
   other. Two scanners = two ProScan processes, two sound devices, two sets of `ProScan.cfg`.
2. **`Main.AudioLoopback` + `Main.AudioTestService`** (`WaveIn.cs:393`) — monitor one service's
   audio through the speakers, where the service is the string `"Source Client"`,
   `"Web Server"`, `"Recorder"`, or `"Remote Scanner Over IP"` mapped to `Type` 1/2/3/4.
   `PlaybackProcessing` (`FrmMain.cs:40372-40392`) then **rejects every other `Type`** —
   so loopback is exclusive, one source at a time.
3. **`PlaybackProcessing`'s re-entrancy guard** (`FrmMain.cs:40367-40371`):

```csharp
if (_0024STATIC_0024PlaybackProcessing_0024205181D6E88_0024running) return;   // DROP, don't queue
_0024STATIC_0024PlaybackProcessing_0024205181D6E88_0024running = true;
```

   Concurrent calls **drop frames on the floor** rather than queueing. With four capture
   channels all calling into one playback path, that's the safety valve that keeps the UI alive
   — and the reason a second scanner's audio can't simply be folded in.

**Implication for your app:** you're building something ProScan deliberately doesn't do. The
useful parts to borrow are the per-channel gain stage (§6, which is exactly the panning
primitive you need) and the winmm ring structure (§3). The parts to *avoid* copying are the
50×/25× oversized rings, the un-checked output ring (§10), and the post-volume record tap.

---

## 10. Bugs and sharp edges worth knowing

1. **The output ring never checks completion.** `waveOutOpen` is called with `CALLBACK_NULL`
   (`WaveOut.cs:92`) and `WaveOutPlay` never tests `WHDR_DONE` — the only guard is
   `lpData == IntPtr.Zero` (`:49`). With `_CountBuffer` wrapping at 25 (`:45`), a producer that
   outruns the device **overwrites a buffer that is still playing**. Symptom: clicks/glitches
   under load. Correct fix: test `WHDR_DONE` per slot, or use `CALLBACK_EVENT` like the capture
   side does.
2. **Playback frames are silently dropped on re-entrancy** (`FrmMain.cs:40367`), not queued.
3. **Recordings are post-volume** (§6) — no independent record gain.
4. **`BufferSize × 2` doesn't match the stereo format** (§4) — effective buffer is half the
   apparent duration.
5. **`ScaleOutVolume(60)` = +30 dB / ~31× gain**, hard-clipped by `LimitShort1`. Nothing warns
   you; you just get distortion.
6. **Capture busy-waits** with `Thread.Sleep(1)` polling `WHDR_DONE` inside the audio thread
   (`WaveIn.cs:295-302`).
7. **Buffer allocations are ~20× the used length** (`nAvgBytesPerSec × 2` vs `BufferSize × 2`).
   Wasteful, not harmful.
8. **`MAXINVOLUMELEVEL` (30) and `MAXOUTVOLUMELEVEL` (60) are both dead** — declared at
   `Main.cs:351`/`:353` and referenced nowhere. The real clamp is hardcoded in
   `ScaleOutVolume`. **There is no input-gain control in software at all** — input level is only
   *displayed* (via `Main.LevelIndicators`, `AudioControl.cs:6949`); you set it in the Windows
   mixer.
9. **`waveInOpen` is retried 5× with no backoff** (`WaveIn.cs:218-233`).
10. **Five playback implementations coexist** (MCI, NAudio `WaveOut`, `DirectSoundOut`, custom
    `WaveOut`, plus `WaveOutEvent` present but unused). Debugging "playback" means first working
    out which one is live.
11. **Mono downmix is a 2-sample average** `(s[i] + s[i-1]) / 2` over the interleaved pair
    (`PlaybackProcesser.cs:418`) — note it reads `BufferStereo[i-1]` for `i` even/odd accordingly,
    so it's a true L+R average, but it's done **after** the per-channel gains, i.e. the downmix
    is gain-weighted.

---

## 11. Practical implications for a dual-scanner app

Since that's your goal, the concrete takeaways:

- **Panning: reuse `ScaleOutVolume` + per-channel gain.** A ±30 dB slider where 30 = 0 dB, 0 =
  hard mute, 1 dB per step, is a sane, proven curve. For scanner A left / scanner B right, give
  each scanner its own gain pair rather than ProScan's single left/right pair.
- **Do not rely on `dwBufferLength = BufferSize × 2` reasoning** — decide explicitly whether
  your buffer size is frames or samples, and make the `× 2` / `× 4` agree with
  `nBlockAlign = 4`.
- **Use `CALLBACK_EVENT` on playback too**, and check `WHDR_DONE`, or you'll inherit bug #1.
- **Tap recordings before the monitoring gain stage** — you want archives at a fixed level
  regardless of what the operator is monitoring at.
- **Your squelch has to be yours.** ProScan proves it's viable to treat audio as a continuous
  stream and drive record start/stop from scanner state, but if you want level-based gating,
  the `ProcessAudio2` pattern (per-buffer energy history + dB conversion with a −90 dB floor +
  `Math.Max` across channels) is the reference implementation to copy.
- **8 kHz mono → 22050 Hz is LAME's `dwReSampleRate`**, not a resampler you have to write, if
  you also encode with `lame_enc.dll` (`beInitStream` / `beEncodeChunk`).

---

## Verify it yourself

```bash
cd ~/workspace/proscan/decompiled
sed -n '174,271p'    WaveIn.cs          # ctor: device open, format, buffer ring, thread start
sed -n '273,326p'    WaveIn.cs          # the drain loop + WHDR_DONE spin
sed -n '328,402p'    WaveIn.cs          # per-buffer DSP chain
sed -n '39,126p'     WaveOut.cs         # the output ring + open flags (CALLBACK_NULL)
sed -n '400,418p'    PlaybackProcesser.cs   # stereo path: per-channel gain + downmix
sed -n '801,817p'    PlaybackProcesser.cs   # mono path: the panning mechanism
sed -n '1119,1134p'  PlaybackProcesser.cs   # ScaleOutVolume curve
sed -n '254,270p'    Common_PS_RF.cs        # LimitShort1
sed -n '100,120p'    ConvertPCMToMP3.cs     # dwReSampleRate
sed -n '32330,32435p' General.cs            # device caps + name->ID
sed -n '40000,40060p' FrmMain.cs            # StartWaveInSC: the real construction args
sed -n '40336,40430p' FrmMain.cs            # PlaybackProcessing: Type routing + drop guard
```

Also grep for the things that **don't** exist, which is how §8 and §9 were settled:

```bash
grep -rn "BufferMilliseconds\|NumberOfBuffers\|DiscardOnBufferOverflow\|BufferedWaveProvider" --include=*.cs . | grep -v ProScan1_dll   # empty
grep -rni "squelch" --include=*.cs . | grep -v ProScan1_dll                                                                              # data-model only
grep -rn "WasapiOut\|WasapiCapture" --include=*.cs . | grep -v ProScan1_dll                                                              # empty
```

Field-check against a real install: the device dropdown contents versus
`waveInGetNumDevs()`, and the `[SOIP LEFT IP LINE LEVEL]` / `[SOIP RIGHT IP LINE LEVEL]` values
in `ProScan.cfg` after moving the volume sliders (note the misleading "SOIP … IP LINE LEVEL"
key names for what is the general speaker output).
