# ProScan virtual keypad & display control — wire strings

Extraction of `RadioSpecific.RS_Keypad()` and the Display-area control handlers in `Display.cs`.
Every string below is quoted verbatim from the decompiled source. Cited by `file:line`.

---

## 0. There are THREE keypad dialects, not one

`RS_Keypad` output is *not* uniform, and a separate `Display.SendKeyType` field selects the
dialect used by the display-area mouse handlers. Both are set per model family.

### Dialect by family — `Display.GetPositions()` (`Display.cs:6103`)

| `SendKeyType` | Families | Wire form |
|---|---|---|
| **1** | `BC780XLT` (`:6112`, set `:6141`); `BC250D`, `BC296D`, `BC785D`, `BC796D` (`:6144-6147`, set `:6181`) | `KEYnn\r` — **no comma** |
| **2** | `BCT15`, `BCT15X`, `BR330T`, `BC346XT`, `BC346XTC`, `BCD325P2`, `BCD396T`, `BCD396XT`, `BCD996T`, `BCD996XT`, **`BCD996P2`**, `BCD160DN`, `BCD260DN`, `BC125AT`, `UBCD160DN`, `UBCD260DN`, `UBC125XLT`, `UBC126AT` (`:6184-6201`, set `:6235`); `BCD536HP`, `UBCD536PT` (`:6238-6239`, set `:6273`); `BCD436HP`, `SDS100`, `SDS150`, `SDS200`, `SDS100E`, `SDS200E`, `UBCD3600XLT`, `USDS100`, `UBCD436PT` (`:6276-6284`, set `:6342`) | `KEY,<c>,P\r` — **comma-separated** |

### The press/hold suffix rule — TWO DIFFERENT CONVENTIONS

| Family group | Press | Hold | Evidence |
|---|---|---|---|
| BC250D / BC296D / BC785D / BC796D | `KEY07` | `KEY07H` | `RadioSpecific.cs:2869-2872` — `if (... && Hold) text += "H";` |
| BC780XLT | `KEY00` | `KEY00H` | same rule (`:2874` block) |
| **BCD996P2 family** | `KEY,P,P` | `KEY,P,L` | `RadioSpecific.cs:3178-3181` — `text = (Hold ? (text + ",L") : (text + ",P"));` |
| **BCD436HP / SDS family** | `KEY,A,P` | `KEY,A,L` | `RadioSpecific.cs:3491-3494` — same rule |

Note this makes the BCD996P2-family press string for button 1 literally **`KEY,P,P`** — the
keyword is `P` and the action suffix is also `P`.

### The legacy hold latch — a real quirk

`BC250D`/`BC296D` (and `BC780XLT`) have a static `holdtemp` that swallows the *next* non-hold
call and returns an empty string (`RadioSpecific.cs:2771-2776`, `:2875-2879`):

```csharp
if (!Hold && _0024STATIC_0024RS_Keypad_002402EE2_0024holdtemp)
{
    _0024STATIC_0024RS_Keypad_002402EE2_0024holdtemp = false;
    return string.Empty;      // the release is deliberately dropped
}
_0024STATIC_0024RS_Keypad_002402EE2_0024holdtemp = Hold;
```

The DMA/HP/SDS families have **no such latch** — they emit `,P` and `,L` as independent commands.

---

## 1. `BCD996P2` block — `RadioSpecific.cs:3083-3182`

Also covers `BCT15`, `BCT15X`, `BCD996T`, `BCD996XT`, `BCD260DN`, `UBCD260DN`.

| Control ID | Keyword | Sent (press) | Sent (hold) |
|---|---|---|---|
| `ScannerKnob1` | `KEY,Q` | `KEY,Q,P` | `KEY,Q,L` |
| `ScannerKnob2` | `KEY,V` | `KEY,V,P` | `KEY,V,L` |
| `ScannerKnob3` | `KEY,F` | `KEY,F,P` | `KEY,F,L` |
| `ScannerButton1` | `KEY,P` | `KEY,P,P` | `KEY,P,L` |
| `ScannerButton2` | `KEY,W` | `KEY,W,P` | `KEY,W,L` |
| `ScannerButton3` | `KEY,G` | `KEY,G,P` | `KEY,G,L` |
| `ScannerButton4` | `KEY,M` | `KEY,M,P` | `KEY,M,L` |
| `ScannerButton5` | `KEY,L` | `KEY,L,P` | `KEY,L,L` |
| `ScannerButton6` | `KEY,1` | `KEY,1,P` | `KEY,1,L` |
| `ScannerButton7` | `KEY,4` | `KEY,4,P` | `KEY,4,L` |
| `ScannerButton8` | `KEY,7` | `KEY,7,P` | `KEY,7,L` |
| `ScannerButton9` | `KEY,.` | `KEY,.,P` | `KEY,.,L` |
| `ScannerButton10` | `KEY,2` | `KEY,2,P` | `KEY,2,L` |
| `ScannerButton11` | `KEY,5` | `KEY,5,P` | `KEY,5,L` |
| `ScannerButton12` | `KEY,8` | `KEY,8,P` | `KEY,8,L` |
| `ScannerButton13` | `KEY,0` | `KEY,0,P` | `KEY,0,L` |
| `ScannerButton14` | `KEY,3` | `KEY,3,P` | `KEY,3,L` |
| `ScannerButton15` | `KEY,6` | `KEY,6,P` | `KEY,6,L` |
| `ScannerButton16` | `KEY,9` | `KEY,9,P` | `KEY,9,L` |
| `ScannerButton17` | `KEY,E` | `KEY,E,P` | `KEY,E,L` |
| `ScannerButton18` | `KEY,S` | `KEY,S,P` | `KEY,S,L` |
| `ScannerButton19` | `KEY,H` | `KEY,H,P` | `KEY,H,L` |

**There is no `ScannerButton20`+ in this block.** Maximum button index is 19.

**The numeric keypad is indexed COLUMN-major.** Buttons 6-9 = `1,4,7,.` (first column),
10-13 = `2,5,8,0` (second column), 14-16 = `3,6,9` (third column). If you drive this family,
your button numbering must follow that physical order, not reading order.

Three knobs exist, and per ProScan's manual (`ProScan_Manual.pdf`, p. ~948) the "main knob" of a
DMA scanner type auto-selects the **function knob**.

---

## 2. `BCD436HP` / `SDS100` / `SDS200` block — `RadioSpecific.cs:3394-3495`

Also covers `SDS150`, `SDS100E`, `SDS200E`, `UBCD3600XLT`, `USDS100`, `UBCD436PT`.

| Control ID | Keyword | Sent (press) | Sent (hold) |
|---|---|---|---|
| `ScannerKnob3` | `KEY,^` | `KEY,^,P` | `KEY,^,L` |
| `ScannerButton1` | `KEY,A` | `KEY,A,P` | `KEY,A,L` |
| `ScannerButton2` | `KEY,B` | `KEY,B,P` | `KEY,B,L` |
| `ScannerButton3` | `KEY,C` | `KEY,C,P` | `KEY,C,L` |
| `ScannerButton4` | `KEY,1` | `KEY,1,P` | `KEY,1,L` |
| `ScannerButton5` | `KEY,2` | `KEY,2,P` | `KEY,2,L` |
| `ScannerButton6` | `KEY,3` | `KEY,3,P` | `KEY,3,L` |
| `ScannerButton7` | `KEY,4` | `KEY,4,P` | `KEY,4,L` |
| `ScannerButton8` | `KEY,5` | `KEY,5,P` | `KEY,5,L` |
| `ScannerButton9` | `KEY,6` | `KEY,6,P` | `KEY,6,L` |
| `ScannerButton10` | `KEY,7` | `KEY,7,P` | `KEY,7,L` |
| `ScannerButton11` | `KEY,8` | `KEY,8,P` | `KEY,8,L` |
| `ScannerButton12` | `KEY,9` | `KEY,9,P` | `KEY,9,L` |
| `ScannerButton13` | `KEY,.` | `KEY,.,P` | `KEY,.,L` |
| `ScannerButton14` | `KEY,0` | `KEY,0,P` | `KEY,0,L` |
| `ScannerButton15` | `KEY,E` | `KEY,E,P` | `KEY,E,L` |
| `ScannerButton16` | `KEY,L` | `KEY,L,P` | `KEY,L,L` |
| `ScannerButton17` | `KEY,Y` | `KEY,Y,P` | `KEY,Y,L` |
| `ScannerButton18` | `KEY,Z` | `KEY,Z,P` | `KEY,Z,L` |
| `ScannerButton19` | `KEY,V` | `KEY,V,P` | `KEY,V,L` |
| `ScannerButton20` | `KEY,F` | `KEY,F,P` | `KEY,F,L` |
| `ScannerButton21` | `KEY,M` | `KEY,M,P` | `KEY,M,L` |

**Two structural differences from §1:**

- **Only `ScannerKnob3` is defined.** `ScannerKnob1` and `ScannerKnob2` are absent for this entire
  family — the HP/SDS units have one knob.
- **The numeric keypad is indexed ROW-major here**: buttons 4-14 = `1,2,3,4,5,6,7,8,9,.,0`
  in order. The opposite convention to the BCD996P2 block. Do not share a keymap between them.

---

## 3. `Display.cs:6038-6101` — these are MENU handlers, not the knob

**Correcting the premise:** the region around 6060-6110 is *not* the rotary-dial handler. It is
two functions that let the mouse manipulate **menu rows on the drawn display**.

### `ChangeMenuItemFeature_MouseDoubleClick` — `Display.cs:6038`

Runs on a double-click on a menu row (guards: display not disabled, right-ish button, `ViewType != 2`,
`ScannerMode == 30`, row bounds per family at `:6052-6062`).

```csharp
if (SendKeyType == 1)
{
    SerialPortSendReceive.KEYtxPoll("KEY04\r");
}
else if (SendKeyType == 2)
{
    SerialPortSendReceive.KEYtxPoll("KEY,E,P\r");
}
```

### `ChangeMenuItemFeature_MouseWheel` — `Display.cs:6076`

```csharp
if (e.get_Delta() > -1)              // wheel UP
{
    if (SendKeyType == 1) KEYtxPoll("KEY08\r");
    else if (SendKeyType == 2) KEYtxPoll("KEY,>,P\r");
}
else                                 // wheel DOWN
{
    if (SendKeyType == 1) KEYtxPoll("KEY07\r");
    else if (SendKeyType == 2) KEYtxPoll("KEY,<,P\r");
}
```

So the menu-wheel commands are `KEY,>` / `KEY,<` on the DMA/HP/SDS families — cursor right/left —
and `KEY08` / `KEY07` on the legacy families.

Manual corroboration: *"Added - Menu items can now be changed using a mouse click or mouse wheel
instead of the main knob"* (`ProScan_Manual.pdf` changelog).

---

## 4. The actual rotary knob — `Display.cs:5399`

```csharp
private void ScannerKnob3_KnobValueChanged(string ScannerControlName)
{
    if (((Control)ScannerKnob3).get_Visible())
    {
        string key = string.Empty;
        int value = ScannerKnob3.Value;
        if (value > _0024STATIC_..._0024int2)
        {
            key = "KEY,>, P\r";          // <-- SPACE between ',' and 'P'
        }
        else if (value < _0024STATIC_..._0024int2)
        {
            key = "KEY,<,P\r";           // correct
        }
        if (value != _0024STATIC_..._0024int2)
        {
            SerialPortSendReceive.KEYtxPoll(key);
        }
        _0024STATIC_..._0024int2 = value;
    }
}
```

### 🐛 BUG: rotating the knob UP sends a malformed command

`"KEY,>, P\r"` contains a **spurious space** before `P`. The correctly-formed string used
everywhere else is `"KEY,>,P\r"`.

**Counted across all 1261 decompiled files:**

| Literal | Occurrences | Where |
|---|---|---|
| `KEY,>, P\r` (space) | **1** | `Display.cs:5407` only — the knob-increment path |
| `KEY,>,P\r` (tight) | **14** | `Display.cs:6008`, `:6090`; `FrmMain.cs:25795`; `WebServer.cs:2565/2571/2947/2953/3350/3356/3752/3758/4157/4163/4544/4550`; `VirtualScanner.cs:1164/1228/1265/1309`; `Shortcuts.cs:652` |
| `KEY,<,P\r` | **14** | all tight — **no space-form exists anywhere** |

The same control is re-implemented with duplicated string literals in four separate files
(`Display.cs`, `VirtualScanner.cs`, `WebServer.cs`, `Shortcuts.cs`) — which is how a single
typo of this kind survives. The decrement path is clean in every copy; the increment path is
wrong in exactly one. So:

| Action | String sent | Well-formed? |
|---|---|---|
| Knob rotate up / increment | `KEY,>, P\r` | ❌ **no — embedded space** |
| Knob rotate down / decrement | `KEY,<,P\r` | ✅ |

**Do not copy this string.** Use `KEY,>,P\r`. If the scanner rejects the space form, ProScan's
"turn the virtual knob clockwise" path is silently broken on these families — worth testing on
hardware, because it's an easy defect to inherit.

### Rotation mechanics

- The control's `Value` changes in **steps of 10** per detent (`Display.cs:5168`, `:5172`, `:5181`, `:5185`).
- **One command is sent per `KnobValueChanged` event** — no acceleration, no rate limit, no debounce,
  no coalescing. Spin the knob and you emit a burst of `KEY,<,P` / `KEY,>, P` with no pacing.
- The handler is only wired while the knob is `Visible` (`:5401`).

ProScan's manual describes rotation modes (`ProScan_Manual.pdf` ~p.948):
> Mode 1 — Any virtual knob is selected with a mouse click then the mouse wheel can rotate the knob.
> Mode 2 — No need to select the knob. Automatically selects the main knob (VFO knob on a non-DMA
> scanner type or the function knob on a DMA scanner type).

---

## 5. The softkeys — `Display.cs:5055` (`Display_MouseDown1`)

The three softkey hit-boxes are **fixed pixel rectangles** on the drawn display
(`Display.cs:956-958`):

```csharp
_softkeyrect_1 = new Rectangle(1, 249, 109, 12);
_softkeyrect_2 = new Rectangle(122, 249, 120, 12);
_softkeyrect_3 = new Rectangle(254, 249, 109, 12);
```

A click inside one routes to `Display_MouseDown1` and dispatches **`Hold: true`**:

```csharp
if (Operators.CompareString(Type, "SoftKey1", true) == 0)
{
    if (_softkeyflag_1)
    {
        _softkeypressed_1 = true;
        ((Control)this).Invalidate();
        if (Main.fMain.Allow(2))
        {
            SerialPortSendReceive.KEYtxPoll(RadioSpecific.RS_Keypad("ScannerButton1", Hold: true));
        }
    }
}
```
…and identically `SoftKey2` → `ScannerButton2`, `SoftKey3` → `ScannerButton3` (`:5069`, `:5081`).

**Softkey clicks therefore send the HOLD form** — `KEY,A,L` on a BCD436HP/SDS100, not `KEY,A,P`.

`_softkeyflag_N` is populated by **reference** from the display renderer
(`DisplaySDSCommon.DrawSDS100(... ref _softkeyflag_1, ref _softkeyflag_2, ref _softkeyflag_3 ...)`,
`Display.cs:1230`), i.e. it is derived from parsing the scanner's own display buffer.

---

## 6. ⚠️ "Avoid" / "Scan" / "Channel Step" — ProScan has no such concepts

This is the important correction. **ProScan never names these functions anywhere.** What it knows
about the softkeys is purely **positional**:

- `_softkeyflag_1/2/3` — is the softkey *active* (parsed from the scanner's display data)
- `_softkeyrect_1/2/3` — where it is on screen
- the **caption** is drawn by `DisplaySDSCommon.DrawSDS100` straight out of `_Data1`, the scanner's
  display buffer

Evidence that there is no semantic layer:

- `grep -rn "Avoid"` across all 1261 decompiled `.cs` files returns **only database/property
  contexts** — `TreeViewSortCustom2_C.cs` (`:248`, `:305`, `:386`, …) and `FrmMain.cs` XML import
  (`:23754`, `:23818`, …) treat `"Avoid"` as a *field name*, and `FrmFindReplace_C.cs` uses it as a
  Find/Replace column value. **There is no Avoid button, no Avoid command, and no Avoid string.**
- `ProScan_Manual.pdf` mentions Avoid only as a database parameter (Avoided Frequencies list,
  the Avoid parameter in the Uniden Database utility, avoided/locked-out nodes shown in red).
  It never documents an on-screen Avoid button.
- `RS_Keypad` receives `"ScannerButtonN"` — an index. It has no idea what the button is for.

**Consequence:** SoftKey1 is "Avoid" or "Scan" or "Channel Step" only because the **scanner's own
firmware** draws that caption in that position at that moment. The softkeys are context-dependent —
the same position sends the same byte string while meaning different things in different menus.
There is no static mapping from meaning → bytes to extract, because none exists upstream of the
scanner.

### How to establish the mapping empirically (the only sound way)

Two options, both device-sourced rather than guessed:

1. **Watch ProScan's own log.** Enable Options → **Logging** (`Test.d()` writes to it), click each
   softkey on the virtual faceplate, and read the exact string ProScan emitted. That pairs the
   caption you saw with the bytes.
2. **Drive the scanner directly and observe the response.** With the scanner on a serial port:

   ```
   printf 'KEY,A,P\r' > /dev/ttyUSB0
   ```

   …then read the reply. Per the poll protocol the scanner answers with `OK`/`NG` plus its display
   buffer, and the display buffer contains the softkey captions — so you can confirm which
   `KEY,x` produced an "Avoid"-labelled state. Repeat for `KEY,B` / `KEY,C`.

Do not assume `A`=Avoid, `B`=Scan, `C`=Channel Step. The order is the scanner's choice and it is
**not** documented, and at least one RadioReference thread reports the SDS100's `KEY` codes/format
differing from the BCD436HP's — while ProScan groups them in **one** case block
(`RadioSpecific.cs:3394-3402`) and sends identical strings to both. That is a plausible latent
compatibility gap worth testing on your own hardware.

---

## 7. Quick reference — every control path

| UI action | Function | String sent (DMA/HP/SDS, `SendKeyType == 2`) |
|---|---|---|
| Click softkey 1/2/3 | `Display_MouseDown1` `:5055` | `RS_Keypad("ScannerButton1/2/3", Hold: true)` → `KEY,A,L` / `KEY,B,L` / `KEY,C,L` |
| Knob rotate **up** | `ScannerKnob3_KnobValueChanged` `:5399` | `KEY,>, P\r` ⚠ → use `KEY,>,P\r` |
| Knob rotate **down** | same | `KEY,<,P\r` |
| Menu row double-click | `ChangeMenuItemFeature_MouseDoubleClick` `:6038` | `KEY,E,P\r` |
| Menu wheel **up** | `ChangeMenuItemFeature_MouseWheel` `:6076` | `KEY,>,P\r` |
| Menu wheel **down** | same | `KEY,<,P\r` |
| Rotary / on-screen knob press | `ScannerKnob3_DoubleClick` `:5391` | `RS_Keypad("ScannerKnob3", Hold: false)` → `KEY,^,P` |

All of these go out through `SerialPortSendReceive.KEYtxPoll(string)` — the dedicated immediate
control channel, **separate** from the poll traffic, and all strings carry a trailing `\r`.
