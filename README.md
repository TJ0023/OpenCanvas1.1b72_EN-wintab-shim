# OpenCanvas 1.1 Pen Fix (Wintab shim)

## What software is this for?
This fix is for **OpenCanvas 1.1b72 (English)**, an old Windows drawing program
from the early 2000s. The program file is:

```
OpenCanvas_1.1b72_EN\OpenCancvas_1.1b72_EN.exe
```
(Yes, the original file name really is spelled "Cancvas".)

OpenCanvas itself is **not** included in this repository. You need your own copy.

## What problem does it fix?
On modern Windows PCs, OpenCanvas detects the drawing tablet pen, but the lines
appear **far away from the pen**, usually much lower on the screen. Pressure
works, but you can't draw where you are pointing.

This happens because OpenCanvas was made for 2000-era tablets and screens. Modern
PCs have things it doesn't understand:
- **Display scaling** (for example 125% or 150% in Windows display settings)
- **More than one monitor**, especially monitors placed above or beside each other
- **Newer tablet drivers** that report the pen position differently

So OpenCanvas and the tablet driver disagree about where the pen is on the screen.

## How does the fix work?
OpenCanvas talks to the tablet through a Windows file called `wintab32.dll`.
This fix is a small replacement `wintab32.dll` that sits in the OpenCanvas folder,
between OpenCanvas and the real tablet driver.

- It passes everything through to the real tablet driver as normal.
- But for every pen movement, it replaces the pen's position with the position of
  the **Windows mouse cursor**. The tablet driver already places the cursor
  exactly under the pen tip, so the line now lands exactly where the pen is.
- Pen **pressure** and **buttons** are not changed.

Nothing is installed into Windows and no system files are changed. To undo the fix,
just delete the file again.

It has been tested with a Wacom tablet. Other tablets that use Wintab drivers
(for example some XP-Pen or Huion models) may also work, but are untested.

## Installation (which files to copy)
You need **2 files**, copied into the **same folder as `OpenCancvas_1.1b72_EN.exe`**:

| File | Where to get it |
|---|---|
| `wintab32.dll` | From the **Releases** page of this repository, or build it yourself (see below). After building it is in `target\i686-pc-windows-msvc\release\` |
| `wintab_shim.ini` | In the main folder of this repository |

After copying, your OpenCanvas folder should look like this:

```
OpenCanvas_1.1b72_EN\
├── OpenCancvas_1.1b72_EN.exe
├── Viewer11b13_EN.exe
├── Readme_2019.txt
├── wintab32.dll        <- added
└── wintab_shim.ini     <- added
```

Then start `OpenCancvas_1.1b72_EN.exe` as usual and draw.

**Do NOT** copy `wintab32.dll` into `C:\Windows\System32` or `C:\Windows\SysWOW64`.
That would replace your real tablet driver. It only belongs in the OpenCanvas folder.

### Uninstall
Delete `wintab32.dll` and `wintab_shim.ini` from the OpenCanvas folder.
(Also delete `wintab_shim.log` if it is there.)

## Settings (`wintab_shim.ini`)
You can open this file in Notepad. Restart OpenCanvas after changing it.

| Setting | Default | What it does |
|---|---|---|
| `mode` | `cursor` | `cursor` = fix is on. `passthrough` = fix is off (for comparison) |
| `offset_x` / `offset_y` | `0` | Moves the line a few pixels if there is still a small, even gap |
| `flip_y` | `false` | Set to `true` if moving the pen up moves the line down |
| `verbose` | `false` | Set to `true` only for troubleshooting. Logs every pen movement |

The fix writes a small `wintab_shim.log` file in the OpenCanvas folder. It
automatically resets itself once it reaches 256 KB, so it never grows large.

## Building it yourself (optional)
Only needed if you don't download `wintab32.dll` from Releases.

1. Install Rust from https://rustup.rs
2. Install the **Visual Studio C++ Build Tools** with the
   "Desktop development with C++" option.
3. Open a terminal in this repository's folder and run:
   ```
   rustup target add i686-pc-windows-msvc
   cargo build --release --target i686-pc-windows-msvc
   ```
4. The file is created at `target\i686-pc-windows-msvc\release\wintab32.dll`.

OpenCanvas 1.1 is a **32-bit** program, so the `i686` (32-bit) target is required.
A normal `cargo build` makes a 64-bit file that OpenCanvas cannot use.

## Troubleshooting
- **Line is still a little off everywhere by the same amount:** adjust `offset_x` / `offset_y`.
- **Up and down are reversed:** set `flip_y=true`.
- **Pen doesn't work at all:** make sure both files are next to the `.exe`, and that
  you used the 32-bit build.
- **Still wrong:** set `verbose=true`, draw a few strokes, close OpenCanvas and
  look at `wintab_shim.log`.
