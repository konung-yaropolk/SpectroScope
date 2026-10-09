//! Build-time configuration: the FFTW availability cfg, and the Windows
//! executable resource.
//!
//! The `fftw` crate is declared as a target-specific optional dependency (it
//! cannot be built for wasm32 and is painful to cross-compile for Android), so
//! `#[cfg(feature = "fftw")]` on its own is not enough: the feature can be
//! active on a target where the crate was never pulled in. This folds the
//! feature flag and the target together into a single `use_fftw` cfg, and all
//! application code keys off that.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    emit_fftw_cfg();
    stage_fftw_runtime_libs();
    embed_windows_resource();
}

/// Put FFTW's shared libraries next to the executable.
///
/// On Windows `fftw-src` installs prebuilt `libfftw3f-3.dll` / `libfftw3-3.dll`
/// into its own `OUT_DIR` and links against the import libraries. Cargo does
/// not copy a dependency's loose DLLs to the output directory, so the binary
/// links fine and then dies at startup with STATUS_DLL_NOT_FOUND (exit code
/// 0xC0000135) before `main` is ever reached. Copying them beside the artifact
/// is what makes a default `cargo run` work.
///
/// Failure is never fatal: the worst case is the same missing-DLL error the
/// developer would have had anyway, and a hard error here would break builds
/// that do not use FFTW at all.
fn stage_fftw_runtime_libs() {
    if std::env::var_os("CARGO_FEATURE_FFTW").is_none() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    // OUT_DIR is <target>/<triple?>/<profile>/build/<pkg>-<hash>/out, so the
    // artifact directory is three levels up. Cargo offers no direct variable
    // for it, and this layout has been stable for the life of the tool.
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(std::path::PathBuf::from) else {
        return;
    };
    let Some(artifact_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let build_dir = artifact_dir.join("build");

    let Ok(entries) = std::fs::read_dir(&build_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("fftw-src-") {
            continue;
        }

        let Ok(files) = std::fs::read_dir(entry.path().join("out")) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("dll") {
                continue;
            }
            let Some(file_name) = path.file_name() else {
                continue;
            };
            let destination = artifact_dir.join(file_name);

            // Skip an up-to-date copy so an incremental build does not keep
            // rewriting a file the linker or a running binary may have open.
            let fresh = std::fs::metadata(&destination)
                .ok()
                .zip(std::fs::metadata(&path).ok())
                .and_then(|(d, s)| Some((d.modified().ok()?, s.modified().ok()?)))
                .is_some_and(|(d, s)| d >= s);
            if fresh {
                continue;
            }

            if let Err(e) = std::fs::copy(&path, &destination) {
                println!("cargo::warning=could not stage {}: {e}", path.display());
            }
        }
    }
}

fn emit_fftw_cfg() {
    println!("cargo::rustc-check-cfg=cfg(use_fftw)");

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let feature_on = std::env::var_os("CARGO_FEATURE_FFTW").is_some();

    let dependency_present = arch != "wasm32" && os != "android";

    if feature_on && dependency_present {
        println!("cargo::rustc-cfg=use_fftw");
    }
}

/// Embed the icon and version strings into the `.exe`.
///
/// `#[cfg(windows)]` in a build script describes the **host**, because that is
/// what the script itself was compiled for -- and so does the
/// `[target.'cfg(windows)'.build-dependencies]` gate that supplies `winres`.
/// The *target* has to be asked about separately: cross-compiling from Windows
/// to wasm or Android still runs this script, and handing a Win32 resource to a
/// non-Windows target just fails.
///
/// This runs after [`emit_fftw_cfg`] and never before it, because it returns
/// early for non-Windows targets and would otherwise swallow the `use_fftw`
/// cfg on, say, a Windows-to-Linux cross build.
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
