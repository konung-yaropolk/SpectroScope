# SpectroScope

Spectrum analyzer for multiple SDR platforms — a Rust rewrite of
[QSpectrumAnalyzer](https://github.com/xmikos/qspectrumanalyzer) on `egui` with a
`wgpu` renderer, so the same code runs on Windows, Linux, macOS, Android and in
a browser.

The GUI follows the original: a spectrum plot over a waterfall, with *Controls*,
*Frequency*, *Settings* and *Levels* panels, and the same measurement features —
max/min peak hold, running average, smoothing, a persistence fan and baseline
subtraction.

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

The waterfall keeps 8192 sweeps by default -- over two hours at one sweep a
second -- and *Settings* raises that to 16384, which is the largest texture
common GPUs allow. Zoomed fully out a pane shows tens of thousands of sweeps at
once, so what bounds the visible span is how much history is kept rather than
the zoom. The history is also capped by memory, since the cost is
`bins x sweeps x 4` bytes and a very wide sweep would otherwise ask for
gigabytes; the row count is reduced to fit and the reduction is logged.

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

- **PNG** is a picture: the colour map baked into 8-bit RGB, for a report or a
  bug thread.
- **TIFF** is the measurement. Pixels are the raw `f32` dB values, written
  through [`fast-tiff-lib`](https://crates.io/crates/fast-tiff-lib) as an
  ImageJ-compatible 32-bit float image. The colour map travels beside the data
  as an ImageJ LUT, along with the display window, so ImageJ opens it looking
  exactly like the screen while *Analyze → Measure* and every plugin still read
  dBm. Baking the colours in would quantise 32-bit data to 8 bits per channel
  and make the file unmeasurable. Axis parameters — start frequency, bin width,
  sweep interval — ride along in the ImageJ description.

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
