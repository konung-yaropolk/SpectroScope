# Building SpectroScope

`rust-toolchain.toml` pins **Rust 1.96**, so rustup installs the right toolchain
on first build and nothing needs selecting by hand.

## Desktop (Windows, Linux, macOS)

```bash
cargo run --release
```

There is no native dependency: the FFT is `rustfft`, so the binary is
self-contained and the same sources build for every target. *Help → About*
reports the FFT library the running build uses.

On Windows `.cargo/config.toml` adds `-C target-feature=+crt-static`, which
links the Microsoft C runtime into the executable. Without it the `.exe`
carries a static import of `VCRUNTIME140.dll` — part of the Visual C++
Redistributable rather than of Windows, so a machine that has not got it
cannot start the program at all. `scripts/pe_imports.py` prints a PE file's
imports and fails on a forbidden one, which is how CI checks that the flag is
still taking effect:

```bash
python scripts/pe_imports.py target/release/spectroscope.exe \
    --forbid VCRUNTIME --forbid api-ms-win-crt
```

Nothing else is linked statically on purpose. On Linux the GUI reaches libGL,
libvulkan, X11/Wayland and the desktop portal through `dlopen`, none of which a
static libc can serve, and macOS has no supported static libSystem; those two
are distributed as self-contained bundles instead (AppImage, Flatpak, `.dmg`).

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
WebGPU is used when the browser has it, and WebGL is the fallback.

Image export works in the browser. `data::image_export::encode` returns the
encoded bytes rather than writing them, so both builds run the same encoder and
only the last step differs: the desktop writes the bytes to the path the user
picked, and `web::download` wraps them in a Blob, gives it an object URL and
clicks a detached `<a download>`. The container still comes from the name's
extension, so the two produce the same two kinds of file. Recording is the half
that does not travel — appending one sweep at a time for hours is what makes an
interrupted capture readable, and a tab has nowhere to append to.

A browser cannot open a raw TCP socket, so the `rtl_tcp` backend talks WebSocket
here and needs a bridge in front of the server, for example:

```bash
websocat --binary ws-l:0.0.0.0:8080 tcp:127.0.0.1:1234
```

Then set *Device* to `ws://localhost:8080`.

## Android

Needs the Android SDK and NDK, a JDK, and
[`cargo-apk2`](https://github.com/mzdk100/cargo-apk2) — the maintained fork of
`cargo-apk`, which has had no release since 2023 and points at `xbuild`, which
now describes itself as unmaintained. `cargo-apk2` reads the same
`[package.metadata.android]` table, so nothing in `Cargo.toml` had to change
for it.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi
cargo install --locked cargo-apk2
cargo apk2 build --lib --release
```

`--lib` matters: the crate has a `[[bin]]` as well as a `[lib]`, and without it
`cargo apk2` would also try to package `src/main.rs`, which is the desktop
entry point. Both ABIs go into one APK, from `build_targets` in the metadata.
A release build needs a signing key — see the `build-android` job in
`.github/workflows/release.yml`, which explains the two ways to supply one.

**Checking it compiles needs none of that.** `cargo check` never links, so the
target's `std` is the only thing required:

```bash
cargo clippy --target aarch64-linux-android --lib -- -D warnings
```

That is what CI runs on every push, for both ABIs. It is worth having, because
`android_main` in `src/lib.rs` is behind `cfg(target_os = "android")` and no
other job compiles a line of it.

A fourth thing had to be fixed before the APK was any *use*, and it is worth
knowing about because nothing warns you loudly: `cargo-apk2` models
`application.activities` as a `Vec` that defaults to empty, and when it is
empty it prints one line to stderr and builds the APK anyway. The manifest then
has no `<activity>`, so the app installs, shows no launcher icon and cannot be
started — which is exactly what release v0.1.0 shipped. The
`[[package.metadata.android.application.activity]]` block in Cargo.toml is what
fixes it; `tests/android_manifest.rs` and `scripts/apk_manifest.py` are what
keep it fixed, the first reading Cargo.toml on every push and the second
decoding the binary manifest out of the built APK during a release.

To see the XML that metadata will actually produce, without an SDK or an NDK,
run `ndk-build2`'s own serializer over it from a throwaway crate:

```rust
// deps: ndk-build2 = "1.4", serde = { version = "1", features = ["derive"] }, toml = "0.8"
#[derive(serde::Deserialize)]
struct Meta { #[serde(flatten)] manifest: ndk_build2::manifest::AndroidManifest }

let doc: toml::Value = toml::from_str(&std::fs::read_to_string("Cargo.toml")?)?;
let meta: Meta = doc["package"]["metadata"]["android"].clone().try_into()?;
meta.manifest.write_to(std::path::Path::new("."))?;   // writes AndroidManifest.xml
```

It is the same code path `cargo apk2 build` takes, so what it prints is what
the APK gets. (It also shows an upstream wart worth not being alarmed by: two
`Option<bool>` application fields are serialized as empty *elements* --
`<android:allowNativeHeapPointerTagging/>` and
`<android:requestLegacyExternalStorage/>` -- because their serde renames are
missing the attribute prefix. aapt2 ignores them.)

Three further things had to be fixed before any of this built, all of them the
kind that only a real cross-compile finds:

- `rfd` has no Android backend — it picks one by `target_os` and knows
  Windows, macOS, the BSDs, Linux and wasm32, so on Android its `backend`
  module compiles empty while `file_dialog.rs` still imports from it. It is
  excluded from the Android dependency graph, and the two call sites return
  `None` there.
- `eframe` re-exports neither `winit` nor `egui-winit`, so the
  `EventLoopBuilderExtAndroid` import the entry point needs comes from a direct
  `winit` dependency.
- `eframe`'s `accesskit` feature is a hard `compile_error!` alongside
  `android-native-activity`, because accesskit's winit adapter only supports
  GameActivity. It is therefore enabled per-target rather than unconditionally.

What the Android build can and cannot do:

- The helper-process backends (`soapy_power`, `hackrf_sweep`, `rtl_power`,
  `rtl_power_fftw`, `rx_power`) compile but cannot run: Android does not let an
  app execute a binary it has written. `rtl_tcp` over the network and `demo`
  are what work, and the manifest already requests `INTERNET`.
- There is no file dialog, so paths for recording and export are typed.
- The launcher icon comes from `packaging/android/res/mipmap-*/ic_launcher.png`,
  downscaled from the committed PNGs. Regenerate them if `icon.svg` changes;
  nothing rasterizes the SVG, because its mesh gradient needs Inkscape.

## Checks

```bash
cargo test --all-targets                        # 377 unit + 5 integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
cargo clippy --target wasm32-unknown-unknown --lib -- -D warnings
cargo clippy --target aarch64-linux-android --lib -- -D warnings
desktop-file-validate packaging/spectroscope.desktop
```

`.github/workflows/ci.yml` runs all of these, split across one job per
operating system because each platform compiles code the others never see.

## The `cdylib` half, and the file it leaves behind

`crate-type = ["lib", "cdylib"]` exists for the two targets that load the
application as a library rather than running it: `wasm-bindgen` links the
`cdylib`, and so does the Android `NativeActivity`. On a desktop target that
same crate type produces a `spectroscope.dll` (or `libspectroscope.so`) beside
the executable which **exports nothing, is loaded by nothing, and should not be
shipped** — the binary links the `lib` half statically, as `cargo` always does.

Naming the binary target skips it entirely:

```bash
cargo build --release --bin spectroscope
```

which is what every build in `.github/workflows/release.yml`, the Arch
`PKGBUILD` and the Flatpak manifest do. `cargo build` without `--bin` is
harmless, just a wasted link.

## Known cosmetic warnings

- `output filename collision ... spectroscope.pdb` on Windows builds. The crate
  is both a `lib`/`cdylib` and a binary of the same name, so MSVC wants one
  `.pdb` path for both. It affects debug symbols only, release builds strip
  them, and the alternative is renaming one of the two — which would mean
  either `spectroscope-lib` in every `use` or an executable not called
  `spectroscope`.
