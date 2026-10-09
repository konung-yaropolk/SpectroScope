# Building SpectroScope

`rust-toolchain.toml` pins **Rust 1.96**, so rustup installs the right toolchain
on first build and nothing needs selecting by hand.

## Desktop (Windows, Linux, macOS)

```bash
cargo run --release
```

FFTW is on by default. On Windows `fftw-src` fetches a prebuilt
`libfftw3f-3.dll`; `build.rs` copies it next to the executable, because Cargo
does not, and without it the binary dies before `main` with
`STATUS_DLL_NOT_FOUND` (`0xC0000135`). **When you ship a Windows build, ship
those DLLs alongside the `.exe`.**

To build without FFTW — smaller, no native dependency, `rustfft` instead:

```bash
cargo run --release --no-default-features
```

*Help → About* shows which FFT library the running build is using.

### Linux dependencies

The usual winit/wgpu set, e.g. on Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
                 libx11-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```

Both X11 and Wayland are enabled.

## Web

Needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/).

```bash
wasm-pack build --target web --out-dir pkg --release
python -m http.server 8777          # or any static server
# open http://localhost:8777
```

`index.html` loads `pkg/spectroscope.js` and mounts the app on its canvas.
WebGPU is used when the browser has it and WebGL is the fallback, so the FFTW
feature is simply ignored for this target and `rustfft` is used — no need to
pass `--no-default-features`.

A browser cannot open a raw TCP socket, so the `rtl_tcp` backend talks WebSocket
here and needs a bridge in front of the server, for example:

```bash
websocat --binary ws-l:0.0.0.0:8080 tcp:127.0.0.1:1234
```

Then set *Device* to `ws://localhost:8080`.

## Android

Needs the Android SDK/NDK and
[`cargo-apk`](https://crates.io/crates/cargo-apk) (or `cargo-ndk` plus your own
Gradle project).

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi
cargo apk run --release
```

FFTW is excluded for Android, so `rustfft` is used. The helper-process backends
are compiled out too — nothing there can spawn `soapy_power` — which leaves
`rtl_tcp` and `demo`. The manifest already requests `INTERNET` and declares
optional USB-host support.

> The Android entry point in `src/lib.rs` is gated behind
> `#[cfg(target_os = "android")]` and has **not** been compiled in this
> environment, since no NDK was available. The desktop and web targets are
> verified; expect to adjust the `android-activity` version to whatever your
> `winit` pulls in if the glue does not line up.

## Checks

```bash
cargo test                                      # 293 unit tests
cargo clippy --all-targets                      # clean
cargo fmt --all --check
cargo check --target wasm32-unknown-unknown
```

## Known cosmetic warnings

- `output filename collision ... spectroscope.pdb` on Windows debug builds. The
  crate is both a `lib`/`cdylib` (needed by wasm-pack and cargo-apk) and a
  binary of the same name, so MSVC wants one `.pdb` path for both. It affects
  debug symbols only, and release builds strip them.
