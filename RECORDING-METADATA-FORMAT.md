# ProScan recording metadata format

Reverse-engineered from the shipped binaries, not from docs. Every claim below carries
its provenance as `file:line` in `decompiled/` (ILSpy 7.2.1 C# output of `ProScan.exe`).

Source files:
- `decompiled/FileRecording.cs` — the recorder/tagger (MP3 + WAV paths)
- `decompiled/FileRecordingUID.cs` — the UID-variant recorder, same format
- `decompiled/General.cs` — `GetFormatters()`, the `%`-specifier template engine

ProScan does **not** use NAudio's ID3 writer. It hand-rolls the bytes. NAudio 1.7.3 is
bundled in `ProScan1.dll` (`ProScan1_dll/NAudio_1_7_3.Wave/Id3v2Tag.cs`) but the recording
path never calls it — everything here is raw `FileStream` writes.

---

## Two containers, one shared payload

| Recorder mode | Container | Metadata goes into |
|---|---|---|
| `_AudioFormatREC == 2` | MP3 (LAME, `lame_enc.dll`) | ID3v2.3 tag at file start |
| `_AudioFormatREC == 1` | PCM WAV | RIFF `LIST`/`INFO` chunk before `fmt ` |

Both paths then append the same proprietary **`pros` chunk** — ProScan's scanner record.

```
WriteMP3ID3Header()   FileRecording.cs:333
WriteWavHeader()      FileRecording.cs:403
WriteUSAPHeader()     FileRecording.cs:459   <- the shared payload
WriteUSAPData()       FileRecording.cs:532
```

---

## 1. The `pros` chunk (the actual scanner metadata)

Written by `WriteUSAPHeader()`. Named "USAP" in the code but the on-disk magic is `pros`.

### Framing

```
offset  size  content
+0      4     "pros"                         ASCII, no NUL
+4      4     uint32 length of payload       little-endian, backpatched
+8      len   payload                       NUL-terminated ASCII key:value strings
```

- The 4-byte length is written as a placeholder, then `_FileStream.Seek(position)` rewrites
  it after the payload is built (`FileRecording.cs:502-505`).
- **`length` counts the payload only** — it excludes the 8 bytes of `pros` + length.
- If the payload length is odd, one `0x00` pad byte is appended and counted
  (`FileRecording.cs:496-501`) so the next RIFF chunk stays 2-byte aligned.

### Payload

A flat run of `Key:Value\0` strings, in this exact order, no count field — you parse until
you've consumed `length` bytes:

| # | Key | Source |
|---|---|---|
| 1 | `Scanner:` | `Main._RadioType` |
| 2 | `EndingDate:` | placeholder `00000000000000`, backpatched at close |
| 3 | `FavoriteName:` | `ScannerData1.FavoriteName` |
| 4 | `SystemName:` | `ScannerData1.SystemName` |
| 5 | `SiteName:` | `ScannerData1.SiteName` |
| 6 | `DepartmentName:` | `ScannerData1.GroupName` |
| 7 | `ChannelName:` | `ScannerData1.ChannelName` |
| 8 | `Frequency:` | `ScannerData1.Frequency` |
| 9 | `Modulation:` | `ScannerData1.Modulation` |
| 10 | `Tone:` | backpatched, pre-sized **24** chars |
| 11 | `TGID:` | `ScannerData1.TalkGroup` |
| 12 | `UID:` | backpatched, pre-sized **64** chars |
| 13 | `UID#:` | backpatched, pre-sized **16** chars |
| 14 | `FTOA:` | `ScannerData1.FTOA` |
| 15 | `FTOB:` | `ScannerData1.FTOB` |
| 16 | `RSSI:` | `Rssi1` or `Rssi2` per `Main.RssiMeterBars` |
| 17 | `AlertColor:` | `ScannerData1.AlertColor` |
| 18 | `ServiceType:` | `ScannerData1.ServiceType` |
| 19 | `DisplayedSystemType:` | `ScannerData1.DisplayedSystemType` |
| 20 | `DigitalStatus:` | `ScannerData1.DigitalStatus` |
| 21 | `DMRSlot:` | `ScannerData1.DMRSlot`, `/` replaced with `S` |
| 22 | `ScannerMode:` | `ScannerData1.InfoMode` up to first `,` |

**Gotcha — fields 10/12/13 are fixed-width padding, not empty.** `Tone`, `UID` and `UID#`
are written space-padded (`PadRight(24/64/16)`) with `NoTrim: true`, because they can't be
known until the transmission ends. `CloseFile(UpdatedTone, UpdatedUID, UpdatedUIDNumber)`
(`FileRecording.cs:557`) seeks back and overwrites them in place. The padding is what keeps
the byte offsets stable. If you write this format, you must reserve the same widths.

**Gotcha — everything else is `Trim()`ed** (`NoTrim: false`), so field order matters but
field lengths vary.

---

## 2. MP3 path — ID3v2.3 tag

`WriteMP3ID3Header()`, `FileRecording.cs:333`.

```
"ID3"          3 bytes
0x03 0x00      2 bytes   version 2.3.0
0x00           1 byte    flags
<4 bytes>      4 bytes   syncsafe size, backpatched (big-endian, 7 bits/byte)
```

The size is patched at file offset 6 with `_FileStream.Length - 10`
(`FileRecording.cs:358-369`), encoded syncsafe.

**The whole tag is only written if at least one of `RecTagMp3TIT2Tag`,
`RecTagMp3TPE1Tag`, `RecTagMp3TPE2Tag` is non-empty** (`FileRecording.cs:337`):

```csharp
if (!string.IsNullOrEmpty(Main.RecTagMp3TIT2Tag.Trim()) ||
    !string.IsNullOrEmpty(Main.RecTagMp3TPE1Tag.Trim()) ||
    !string.IsNullOrEmpty(Main.RecTagMp3TPE2Tag.Trim()))
```

It is an **OR across the three**. Clearing all three means **no ID3 tag at all — and therefore no
`pros` chunk either**, because the chunk is emitted inside that same block. Setting only `TIT2`
still writes `TPE1` and `TPE2` as *frames* — just with empty payloads (declared size 2:
the encoding byte plus the NUL terminator).

> ### ⚠️ WAV has NO equivalent gate — the metadata is asymmetric
> `WriteWavHeader()` (`FileRecording.cs:403`) writes `RIFF`/`WAVE` + the `LIST`/`INFO` chunk +
> `IPRD`/`ICRD`/`INAM`/`IART` + the `pros` chunk **unconditionally**. There is no
> "are any templates set?" test anywhere on the WAV path.
>
> Practical consequence: a user who leaves `TIT2`/`TPE1`/`TPE2` **and** `INAM`/`IART` all empty
> gets **full metadata in `.wav` files and none at all in `.mp3` files**. If you rely on the
> `pros` chunk being present, do not assume it is present in MP3 unless at least one MP3 tag
> template is non-empty.

> ### Tags are written once, at recording START
> `WriteMP3ID3Header()` is called only from `OpenFileMP3()` (`FileRecording.cs:179`) and
> `WriteWavHeader()` only from `OpenFilePCM()` (`:155`) — both at **file creation**. The
> templates are evaluated once, against `ScannerData1` as it stands at that instant, then frozen.
> See `FORMATTERS-AND-FILENAME-SCHEME.md` §5 for what changes at close instead (only the four
> placeholder fields inside the `pros` chunk).

### Frames, in order

```
WriteMP3TextFrame()  FileRecording.cs:375
```

| Frame | Value |
|---|---|
| `TSEE` | literal `"ProScan"` |
| `TRDA` | `yyyyMMddHHmmss-00000000000000` |
| `TIT2` | `GetFormatters(RecTagMp3TIT2Tag)` — Title |
| `TPE1` | `GetFormatters(RecTagMp3TPE1Tag)` — Artist |
| `TPE2` | `GetFormatters(RecTagMp3TPE2Tag)` — Band |
| `pros` | the chunk from §1 (raw, not an ID3 frame) |

Text is `CleanASCII_1()`-filtered and hard-truncated to **253** chars.

### Byte layout per frame

Standard ID3v2.3 — no deviation. `WriteMP3TextFrame` writes the header bytes in an odd
source order (1 byte, then 2 bytes), but all three are `0x00`, so on disk they are
indistinguishable from the canonical `flags(2) + encoding(1)`:

```
4 bytes   frame ID (ASCII)
4 bytes   size, big-endian 32-bit, = text_length + 2
2 bytes   flags = 0x00 0x00
1 byte    encoding = 0x00   (ISO-8859-1)
N bytes   text (ASCII, 1 byte per char)
1 byte    0x00   terminator
```

`size = N+2` is exactly the payload after the 2 flag bytes: `encoding(1) + text(N) + NUL(1)`.
Total frame on disk = `N + 12`. Verified by round-trip against `make_sample.py`, which
mirrors the writer: sequential frame iteration lands on every frame with no drift.

The two non-standard bits that remain are the frame **IDs** `TSEE` and `TRDA` — ID3v2.4
spells these `TSSE` and `TDRC`, so strict validators will flag them. Nothing else about
the tag deviates.

---

## 3. WAV path — RIFF `LIST`/`INFO`

`WriteWavHeader()`, `FileRecording.cs:403`.

Chunk order on disk (confirmed by the write sequence):

```
"RIFF" <size> "WAVE"
"LIST" <infoSize> "INFO"
    "IPRD" "ProScan"
    "ICRD" <yyyyMMddHHmmss-00000000000000>
    "INAM" GetFormatters(RecTagWavINAMTag)
    "IART" GetFormatters(RecTagWavIARTTag)
    <pros chunk from §1>
"fmt " <16> <WAVEFORMATEX>
"data" <size> <PCM>
```

Each `INFO` sub-chunk (`WriteWAVTextFrame`, `FileRecording.cs:509`):

```
4 bytes   chunk ID ("IPRD"/"ICRD"/"INAM"/"IART")
4 bytes   int32, length of the string INCLUDING its NUL terminator
N bytes   string, NUL-terminated, padded to even length
```

> **⚠ Confirmed quirk — the `LIST` size is understated.** The size patched at offset 16
> (`FileRecording.cs:425-428`) accumulates only the `INFO` sub-chunks
> (`Length += Text.Length + 8`, `FileRecording.cs:528`). It **excludes** the 4-byte
> `"INFO"` tag and the entire `pros` chunk. Reproduced in `make_sample.py` and confirmed:
> a chunk walker that trusts the declared size lands mid-`pros` and reads audio as a chunk
> header.
>
> **Any parser must resync**: after consuming `INFO` + the `pros` chunk, continue from the
> end of `pros`, not from `LIST offset + 8 + declared size`. `proscan_tags.py` does this.
> Do not "fix" the size when writing — reproduce it if you want byte-identical output.

`len` counts the terminator, and the pad byte for odd lengths is added to the string
*before* the length is computed, so `len` is always even here.

> **⚠ Confirmed quirk — `RIFF` size and `data` size are written as `0` and never patched.**
> The recorder contains exactly three seek-back operations, all shown above: the ID3 tag
> size (`Seek(6)`), the `LIST` size (`Seek(16)`), and the `pros` length. Nothing ever
> revisits offset 4 (`RIFF` size) or the `data` chunk size — both are written as `0` by
> `WriteWavHeader` (`FileRecording.cs:410`, `FileRecording.cs:456`) and left that way.
>
> Consequence: a ProScan WAV is **not** a spec-clean RIFF file. Players cope by reading to
> EOF. Your tool must derive duration from `FileStream.Length`, and must not treat
> `data size == 0` as "empty file".

---

## 4. Append / re-tag behaviour

`OpenFilePCM()`, `FileRecording.cs:102`.

If the target file already exists, ProScan **appends** rather than overwriting:

1. `GetWAVTextTagsLength(fileStream)` computes where the tag area ends.
2. It seeks there and expects `"fmt "` — if absent, aborts with
   `"Corrupted or not a WAV file — Not recording"` (`FileRecording.cs:140`).
3. It then compares the existing `WAVEFORMATEX` (`nChannels` at tag+10, `nSamplesPerSec` at
   tag+12) against the current settings; mismatch aborts with
   `"Mismatch Channels or Samples Per Second — Not recording"` (`FileRecording.cs:146`).
4. Only then does it open `FileMode.Append`.

MP3 has no such guard — `OpenFileMP3()` just appends (`FileRecording.cs:168`), so an
existing file keeps its original tag and the new audio is concatenated after it.

---

## 5. Template specifiers (`GetFormatters`)

`General.cs:33805`. Applied to the Title/Artist/Band and `INAM`/`IART` templates before
`CleanASCII_1`. Naive sequential `String.Replace` — **longest-prefix specifiers are
replaced first only where they are listed first**, so ordering matters and there is no
escaping.

| Spec | Expands to | Spec | Expands to |
|---|---|---|---|
| `%FT` | freq/TGID display (`GetFreqTGID1`) | `%C` | ChannelName |
| `%TG` | TalkGroup | `%F` | Frequency + DMRSlot |
| `%L1` | ChannelName | `%TA` / `%TB` | FTOA / FTOB |
| `%L2` | SystemName | `%T` | Tone |
| `%ST1` | ServiceType | `%MD` | Modulation |
| `%STC` | ServiceColor | `%R` | RSSI (Rssi1/Rssi2) |
| `%AC` | AlertColor | `%U1` `%U2` `%U3` | UID1 / UID2 / UID3 |
| `%SC` | `Main.InfoSCState` | `%P` | CommPort |
| `%ST` | RadioType | `%D` | date, `MonthDayYearFormat` |
| `%DD` | day `dd` | `%M` | month `MM` |
| `%DT` | full datetime | `%Y` | year `yy` |
| `%TT` | time | `%H` | hour `HH` |
| `%MM` | month `MMM` | `%YY` | year `yyyy` |
| `%AF1` / `%AF2` | program path / filename | `%FA` | FavoriteName |
| `%SI` | SiteName | `%G` `%B` `%DE` | GroupName (all three) |
| `%S` | SystemName | | |

Note `%SC` is `InfoSCState`, and `%P` is the serial port name, not a frequency.

---

## Verify it yourself

```bash
python3 ~/workspace/proscan/proscan_tags.py <file.mp3|file.wav>
python3 ~/workspace/proscan/proscan_tags.py <file> --hex   # dump the raw pros chunk
```

Point it at a recording ProScan actually produced and compare against this document.
Anything that disagrees with §1 with a real file wins over the source reading above.
