//! Build-time configuration: the Windows executable resource.
//!
//! The FFT is pure Rust (`rustfft`), so there is no native library to find,
//! stage or gate a cfg on, and every target builds from the same sources.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    embed_windows_resource();
}

/// Embed the icon and version strings into the `.exe`.
///
/// `#[cfg(windows)]` in a build script describes the **host**, because that is
/// what the script itself was compiled for -- and so does the
/// `[target.'cfg(windows)'.build-dependencies]` gate that supplies `winres`.
/// The *target* has to be asked about separately: cross-compiling from Windows
/// to wasm or Android still runs this script, and handing a Win32 resource to a
/// non-Windows target just fails.
#[cfg(windows)]
fn embed_windows_resource() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || !matches!(target_env.as_str(), "gnu" | "msvc") {
        return;
    }

    println!("cargo::rerun-if-changed=icon/icon.ico");
    if !std::path::Path::new("icon/icon.ico").exists() {
        return;
    }

    // `WindowsResource::new()` picks the string properties up from
    // `[package.metadata.winres]`, so only the icon needs pointing at. The
    // working directory is the crate root.
    let mut res = winres::WindowsResource::new();
    res.set_icon("icon/icon.ico");
    if let Err(e) = res.compile() {
        // A missing Windows SDK must not fail the build; the program is
        // perfectly usable without an embedded icon.
        println!("cargo::warning=winres: could not embed icon/metadata: {e}");
    }
}

#[cfg(not(windows))]
fn embed_windows_resource() {}
