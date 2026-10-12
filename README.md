# SpectroScope
[![Stand With Ukraine](https://raw.githubusercontent.com/vshymanskyy/StandWithUkraine/main/banner-direct-single.svg)](https://stand-with-ukraine.pp.ua)

[![Try it online](https://img.shields.io/badge/Try%20it-SpectroScope%20Online-5b9dd9?logo=googlechrome&logoColor=white)](https://konung-yaropolk.github.io/SpectroScope/)
[![Release](https://img.shields.io/github/v/release/konung-yaropolk/SpectroScope?label=release)](https://github.com/konung-yaropolk/SpectroScope/releases)
[![License](https://img.shields.io/badge/license-%20%20GNU%20GPLv3%20-green)](LICENSE)
[![Build](https://img.shields.io/github/actions/workflow/status/konung-yaropolk/SpectroScope/release.yml?label=build)](https://github.com/konung-yaropolk/SpectroScope/actions/workflows/release.yml)
[![Tests](https://img.shields.io/github/actions/workflow/status/konung-yaropolk/SpectroScope/ci.yml?branch=main&label=tests)](https://github.com/konung-yaropolk/SpectroScope/actions/workflows/ci.yml)

Spectrum analyzer for multiple SDR platforms — a Rust rewrite of
[QSpectrumAnalyzer](https://github.com/xmikos/qspectrumanalyzer) on `egui` with a
`wgpu` renderer, so the same code runs on Windows, Linux, macOS, Android and in
a browser.

The GUI follows the original: a spectrum plot over a waterfall, with *Controls*,
*Frequency*, *Settings* and *Levels* panels, and the same measurement features -
max/min peak hold, running average, smoothing, a persistence fan and baseline
subtraction.

## [SpectroScope Online](https://konung-yaropolk.github.io/SpectroScope/) — no install

The same application, compiled to WebAssembly and rendered by your GPU through
WebGPU (WebGL where that is missing). Nothing is uploaded and there is no
server: the page is static, and every sweep is computed in the tab.

The `demo` backend makes it usable with no hardware at all. Real hardware works
too, through the `rtl_tcp` backend — but a browser cannot open a raw TCP socket,
so that needs a WebSocket bridge in front of the server (see
[BUILDING.md](BUILDING.md#web)).

**Saving the waterfall works here too**, as a download: the same encoder runs in
the tab and the browser is handed the finished PNG or TIFF, extension first —
so the ImageJ file a browser produces is the one the desktop produces. What a
tab cannot do is *record*: writing a sweep at a time for hours needs somewhere
to append to, which is the whole reason an interrupted capture is still
readable, and that stays desktop-only.

Needs a current browser with WebGPU or WebGL2 (Chrome/Edge 113+, Firefox 141+).

## Download

Every link below points at the **latest release**; the
[Releases](https://github.com/konung-yaropolk/SpectroScope/releases) page has older ones.

| Download | For | Install / run |
|----------|-----|---------------|
| [**Installer**](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-setup.exe) or [**Portable**](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope.exe) | <img width="16" height="16" alt="windows" src="https://github.com/user-attachments/assets/506a3842-c123-4f21-b571-ef9769573b04" />&nbsp;**Windows 10 / 11** — 64-bit | The installer adds a Start-menu entry; the portable `.exe` just runs. Neither needs the Visual C++ Redistributable. |
| [`SpectroScope-universal.dmg`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-universal.dmg) | <img width="16" height="16" alt="apple" src="https://github.com/user-attachments/assets/0d2c799e-8aac-4e07-9ee8-96bbccff1693" />&nbsp;**macOS 11+** — Apple Silicon *and* Intel | Open the `.dmg`, drag **SpectroScope** into Applications. |
| [`SpectroScope-android.apk`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-android.apk) | <img width="16" height="16" alt="android" src="https://cdn.simpleicons.org/android/3DDC84" />&nbsp;**Android 8+** — ARM64 *and* ARM32 | Allow "install from unknown sources", then open the file. See [the Android notes](#android) first. |
| [`SpectroScope-x86_64.deb`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-x86_64.deb) | <img width="16" height="16" alt="debian" src="https://github.com/user-attachments/assets/2fefe976-d357-4abb-a476-6a8a31b422fe" />&nbsp;**Debian / <img width="16" height="16" alt="ubuntu" src="https://github.com/user-attachments/assets/e2bd815d-709d-4571-9665-f957c90e8300" />&nbsp;Ubuntu** — x86-64 | `sudo apt install ./SpectroScope-x86_64.deb` |
| [`SpectroScope-arm64.deb`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-arm64.deb) | <img width="16" height="16" alt="debian" src="https://github.com/user-attachments/assets/2fefe976-d357-4abb-a476-6a8a31b422fe" />&nbsp;**Debian / <img width="16" height="16" alt="ubuntu" src="https://github.com/user-attachments/assets/e2bd815d-709d-4571-9665-f957c90e8300" />&nbsp;Ubuntu** — ARM64 | `sudo apt install ./SpectroScope-arm64.deb` |
| [`SpectroScope-x86_64.rpm`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-x86_64.rpm) | <img width="16" height="16" alt="fedora" src="https://github.com/user-attachments/assets/97817114-81e7-40bd-8b4d-165fc856cba4" />&nbsp;**Fedora / <img width="16" height="16" alt="redhat" src="https://github.com/user-attachments/assets/aaf64e10-797c-4cb7-87d9-04293d761cc8" />&nbsp;RHEL / <img width="16" height="16" alt="suse" src="https://github.com/user-attachments/assets/2f9e5316-fbea-4394-b801-784672831993" />&nbsp;openSUSE** — x86-64 | `sudo dnf install ./SpectroScope-x86_64.rpm` |
| [`SpectroScope-arm64.rpm`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-arm64.rpm) | <img width="16" height="16" alt="fedora" src="https://github.com/user-attachments/assets/97817114-81e7-40bd-8b4d-165fc856cba4" />&nbsp;**Fedora / <img width="16" height="16" alt="redhat" src="https://github.com/user-attachments/assets/aaf64e10-797c-4cb7-87d9-04293d761cc8" />&nbsp;RHEL / <img width="16" height="16" alt="suse" src="https://github.com/user-attachments/assets/2f9e5316-fbea-4394-b801-784672831993" />&nbsp;openSUSE** — ARM64 | `sudo dnf install ./SpectroScope-arm64.rpm` |
| [`SpectroScope-x86_64.pkg.tar.zst`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-x86_64.pkg.tar.zst) | <img width="16" height="16" alt="archlinux" src="https://github.com/user-attachments/assets/9bbaa231-f10d-4642-a9a9-a31a8a980e3d" />&nbsp;**Arch Linux** — x86-64 | `sudo pacman -U ./SpectroScope-x86_64.pkg.tar.zst` |
| [`SpectroScope-x86_64.flatpak`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-x86_64.flatpak) | <img width="16" height="16" alt="linux" src="https://github.com/user-attachments/assets/78810b28-56ac-4d72-b0b1-ad8a93b9d2a9" />&nbsp;**Any Linux** — Flatpak, x86-64 | `flatpak install --user ./SpectroScope-x86_64.flatpak` |
| [`SpectroScope-arm64.flatpak`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-arm64.flatpak) | <img width="16" height="16" alt="linux" src="https://github.com/user-attachments/assets/78810b28-56ac-4d72-b0b1-ad8a93b9d2a9" />&nbsp;**Any Linux** — Flatpak, ARM64 | `flatpak install --user ./SpectroScope-arm64.flatpak` |
| [`SpectroScope-x86_64.AppImage`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-x86_64.AppImage) | <img width="16" height="16" alt="linux" src="https://github.com/user-attachments/assets/78810b28-56ac-4d72-b0b1-ad8a93b9d2a9" />&nbsp;**Any Linux** — AppImage, x86-64 | `chmod +x SpectroScope-x86_64.AppImage && ./SpectroScope-x86_64.AppImage` |
| [`SpectroScope-arm64.AppImage`](https://github.com/konung-yaropolk/SpectroScope/releases/latest/download/SpectroScope-arm64.AppImage) | <img width="16" height="16" alt="linux" src="https://github.com/user-attachments/assets/78810b28-56ac-4d72-b0b1-ad8a93b9d2a9" />&nbsp;**Any Linux** — AppImage, ARM64 | `chmod +x SpectroScope-arm64.AppImage && ./SpectroScope-arm64.AppImage` |

**Which one?** On Linux the **.deb** / **.rpm** / **pacman** packages integrate
with your system package manager; the **Flatpak** and **AppImage** are
distro-agnostic and self-contained, which is what you want when no native
package matches your distro. For the CPU: 64-bit Intel/AMD takes the `x86_64`
files, 64-bit ARM (Raspberry Pi, Ampere) the `arm64` ones — `uname -m` prints
`x86_64` or `aarch64`, and `aarch64` is the `arm64` file. **On macOS there is
nothing to choose**: the `.dmg` holds a universal binary and macOS picks the
right half at launch. After installing on Linux, start it from the apps menu or
by running `spectroscope`.

The desktop builds have no runtime dependency of their own — the FFT is pure
Rust and the Windows build links the C runtime statically. What they do need is
a working GPU driver: wgpu takes Vulkan where it finds one and falls back to
OpenGL ES.

**First-launch security prompt** (the binaries are not signed with a paid
developer certificate):

- **Windows** — SmartScreen may warn: *More info → Run anyway*.
- **macOS** — Gatekeeper blocks unsigned apps the first time: **right-click the
  app → Open** (then confirm), or run
  `xattr -dr com.apple.quarantine /Applications/SpectroScope.app` once.

### Android

The APK carries both ABIs, so one file covers every phone. Three things are
worth knowing before you install it:

- **v0.1.0's APK does not work — skip it.** Its generated manifest had no
  `<activity>` element, so it installed, showed no launcher icon and could not
  be started. The cause was a packaging default (`cargo-apk2` treats a missing
  activity as a warning, not an error); it is fixed, the manifest is now
  written out in full, and the release workflow decodes the finished APK and
  fails if the launcher wiring or the icon is missing again.
- It is **signed with a throwaway key** generated during the build, unless the
  repository has a release keystore configured. It installs, but Android will
  refuse to upgrade over an APK from an earlier release, because the keys
  differ — uninstall the old version first.
- The **helper-process backends do not work**. `soapy_power`, `hackrf_sweep`,
  `rtl_power`, `rtl_power_fftw` and `rx_power` are separate programs that
  Android will not let an app execute. Use `rtl_tcp` over the network, which
  needs no helper and is how you reach real hardware here, or `demo`.
- There is **no file dialog**, so paths for recording and export have to be
  typed rather than browsed.

Not listed (32-bit x86, or another CPU)? Build from source — see
[BUILDING.md](BUILDING.md).

## Backends

| Backend | Needs | Platforms |
|---|---|---|
| `soapy_power` | [soapy_power](https://github.com/xmikos/soapy_power) (SoapySDR: RTL-SDR, HackRF, Airspy, SDRplay, LimeSDR, bladeRF, USRP…) | desktop |
| `hackrf_sweep` | [hackrf tools](https://github.com/greatscottgadgets/hackrf) | desktop |
| `rtl_power_fftw` | [rtl-power-fftw](https://github.com/AD-Vega/rtl-power-fftw) | desktop |
| `rtl_power` | [Keenerd's rtl-sdr fork](https://github.com/keenerd/rtl-sdr) | desktop |
| `rx_power` | [rx_tools](https://github.com/rxseger/rx_tools) | desktop |
| `rtl_tcp` | an `rtl_tcp` server — **new, no helper binary needed** | all, incl. web |
| `demo` | nothing — **new, synthetic signal** | all, incl. web |

The first five drive the same helper processes the Python version did and parse
their output formats unchanged.

**`rtl_tcp` is new.** It speaks the rtl_tcp protocol directly: it hops the tuner
itself, reads raw IQ, and computes the PSD in-process with Welch averaging over
`rustfft`. Set *Device* to `host:port` (default port 1234). In the
browser a raw TCP socket is impossible, so the web build talks WebSocket and
needs a WebSocket-to-TCP bridge in front of the server; the backend says so in
the log when it starts.

**`demo` is new** too, and generates a synthetic spectrum so the application is
usable with no hardware — which is the only way the web and Android builds are
useful out of the box.

## Adding a device

Backends are a trait, not a hard-coded list. Implement
[`SpectrumSource`](src/sources/mod.rs), push `SourceEvent`s into the `EventSink`
you are handed, and add the type to `registry()`. The GUI picks up the new
backend's name, its parameter limits and its defaults automatically — there is
nothing to change in the UI. The module documentation walks through a complete
minimal example.

## Using the display

The spectrum and the waterfall share one frequency axis and are aligned to the
pixel, so a feature lines up vertically between them.

| Gesture | Effect |
|---|---|
| *Start* / *Stop* | one button, labelled for what it will do |
| *View -> Frequency ruler* | the yellow `rtl_power`-style ruler above the waterfall |
| drag on the spectrum | pan both views |
| scroll on the waterfall | move back and forth through history |
| **shift** + scroll on the waterfall | stretch time vertically |
| **ctrl** + scroll on the waterfall | zoom the frequency axis about the cursor |
| drag on the waterfall | pan frequency and time together |
| double-click the waterfall | back to live, default scale |
| *Reset view* | fit the spectrum to the sweep and return the waterfall to live |

The vertical scale fits whatever has been captured to the pane, so the
waterfall is full and visibly scrolling from the first sweeps rather than
creeping down a couple of pixels at a time; once there is more history than
pixels it settles at one sweep per row and simply scrolls. Stretching or
scrolling hands the scale to you, and *Reset view* hands it back.

Scrolled away from the newest sweep, the waterfall shows a `history -N s` badge
so a paused-looking display is never mistaken for a stalled one.

The waterfall keeps **16384 sweeps** by default, the most a common GPU's
texture limit allows -- over four hours at one sweep a second. Settings are
versioned, so a config written by an earlier build is brought up to that depth
on first load rather than being pinned to whatever default it was created
with; the migration only ever raises the value. Zoomed fully out a pane shows tens of thousands of sweeps at
once, so what bounds the visible span is how much history is kept rather than
the zoom. The history is also capped by memory, since the cost is
`bins x sweeps x 4` bytes and a very wide sweep would otherwise ask for
gigabytes; the row count is reduced to fit and the reduction is logged. The
same ceiling bounds VRAM, since the waterfall texture mirrors the ring.

## Recording and export

The *Recording* panel writes sweeps as they arrive and saves the waterfall as an
image. Both need a filesystem, so they are desktop-only.

Recording is **append-only and flushed per sweep**, because the case worth
designing for is the capture that gets interrupted. Neither format has a
trailer or an index written at the end, so a file that simply stops — power
cut, closed lid, killed process — is still a valid file holding every sweep
before that point, and at most the sweep in flight is lost.

| Format | Why |
|---|---|
| `soapy_power` binary (`.bin`) — **default** | Same container `soapy_power -F soapy_power_bin` produces, so a recording loads straight back as a *Baseline*. Full `f32` precision, about six times smaller than text, and its records are magic-delimited so a truncated tail is detected rather than misparsed. |
| CSV (`.csv`) | `rtl_power` column order, for reading in other tools. Line-based, so a partial final line is equally harmless. |

*Save waterfall...* writes the stored history at native resolution, one pixel
per bin per sweep, not a screenshot of the pane. The extension picks between
two deliberately different things:

- **PNG** is a picture: the colour map baked into 8-bit RGB, wrapped in
  labelled margins — a yellow frequency ruler across the top in the style of
  `rtl_power`'s `heatmap.py`, and wall-clock times down the left, with the
  capture date in the corner. Tick spacing follows a 1-2-5 ladder, and the SI
  prefix is the smallest that keeps the largest label under five digits, so a
  wide sweep reads `200M … 1600M` rather than flipping to `1.6G` partway
  along. Labels gain a tenths place when sweeps come faster than a second.
- **TIFF** is the measurement. Pixels are the raw `f32` dB values, written
  through [`fast-tiff-lib`](https://crates.io/crates/fast-tiff-lib) as an
  ImageJ-compatible 32-bit float image. The colour map travels beside the data
  as an ImageJ LUT, along with the display window, so ImageJ opens it looking
  exactly like the screen while *Analyze → Measure* and every plugin still read
  dBm. Baking the colours in would quantise 32-bit data to 8 bits per channel
  and make the file unmeasurable. It is also why the TIFF carries no drawn
  ruler by default: painting labels into it would overwrite measurements with
  decoration. Axis parameters — start frequency, bin width, sweep interval —
  ride along in the ImageJ description instead.

  *Label the TIFF too* overrides that when you want one file that is both.
  The ruler and band plan are then written as the display window’s own
  extremes, so the stored LUT paints them at the two ends of the colour map
  and the file is still nothing but sample values — but they do overwrite the
  pixels they cover. The margins shift the data away from the image origin, so
  `spectroscope.data_x0`, `data_y0`, `data_width` and `data_height` record
  where it actually starts.

Both are lossless, which matters when the image is evidence: JPEG ringing
around a carrier is indistinguishable from a spur.

## Performance notes

The rewrite is not a transliteration; the parts that were slow are the parts
that changed:

- **The waterfall is a GPU ring buffer.** One `R32Float` texture holds the raw
  dB values and a new sweep uploads exactly one row. Level windowing and the
  colour map are a fragment shader over a 256-entry LUT, so changing either is
  free and no CPU re-colouring ever happens. The Python version rebuilt and
  re-levelled the whole image every sweep.
- **The history is a real ring buffer.** The original called `np.roll()` on
  every append, copying the entire `rows × bins` array each time.
- **Curves are decimated to ~2 points per pixel** with min/max bucketing, which
  keeps single-bin carriers and notches visible while sending a plot a few
  thousand points instead of a few hundred thousand.
- **Plot point buffers are cached** and rebuilt only when the series they come
  from actually changed.

Colour maps are the baked matplotlib family (magma, plasma, inferno, viridis)
plus turbo, bone and summer. The *Levels* histogram and the colour strip below
it share one dB axis, so the level handles line up on both and the strip shows
the colour each value currently maps to.

The FFT is `rustfft`. The original toolchain used FFTW, but only inside the
helper processes it drove; nothing here has to match it, and a pure-Rust
transform builds unchanged for wasm and Android and performs comparably at the
power-of-two sizes this application plans.

## Deliberate differences from the Python version

Two are worth knowing about, because they change numbers on screen:

- **Smoothing edges.** `utils.smooth()` reflected the signal through its end
  points but took its padding indices from the SciPy cookbook's mirror-padding
  recipe, which is off by one for that reflection. A straight line was therefore
  not a fixed point of the filter and the outermost bins drooped by
  `slope × 0.4` for an 11-tap Hann window — worst at the band edges, where sweep
  data is steepest. SpectroScope uses the indices the reflection calls for, so
  traces differ by up to that amount within one window length of each edge, and
  nowhere else.
- **Recalculated averages.** When smoothing was toggled, the Python
  `recalculate_data()` seeded the average with the newest row and then folded
  the rest in with a weight of zero on the first step, discarding the seed.
  SpectroScope computes the plain mean of every row, which is what both branches
  were meant to produce.

Also: backend `stderr` is captured and shown in *View → Backend log* instead of
going to a console the user may never see.

## Building

See [BUILDING.md](BUILDING.md). In short:

```bash
cargo run --release
```

## License

GPL-3.0-or-later, as QSpectrumAnalyzer is.
