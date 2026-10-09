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
FFTW (or rustfft). Set *Device* to `host:port` (default port 1234). In the
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
plus turbo, bone and summer.

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
