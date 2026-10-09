//! Desktop entry point.
//!
//! Android and the web own their event loops, so their entry points live in
//! `lib.rs`; this file is compiled out entirely for wasm32.

// Keep the console window hidden on Windows release builds, the way
// QSpectrumAnalyzer did with `windows.set_attached_console_visible(False)` --
// except that a linker subsystem flag cannot leave a zombie console behind.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use spectroscope::{SpectroScopeApp, APP_ID, APP_NAME, VERSION};

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{APP_NAME} {VERSION}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let debug = args.iter().any(|a| a == "--debug");
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(if debug {
        "debug"
    } else {
        "info"
    }))
    .init();

    log::info!(
        "{APP_NAME} {VERSION} (FFT: {})",
        spectroscope::dsp::fft_backend_name()
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 892.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title(format!("{APP_NAME} {VERSION}"))
            .with_icon(load_icon()),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(SpectroScopeApp::new(cc)))),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn print_help() {
    println!(
        "\
{name} {version} -- spectrum analyzer for multiple SDR platforms

Usage: spectroscope [OPTIONS]

Options:
      --debug      Verbose logging
  -h, --help       Print this help
  -V, --version    Print the version

Backends are chosen in File -> Settings. Settings are stored under the
platform configuration directory.",
        name = spectroscope::APP_NAME,
        version = spectroscope::VERSION,
    );
}

/// The window and taskbar icon.
///
/// Decoded from the same `icon/` artwork that `build.rs` embeds into the `.exe`
/// resource, so the title bar, the taskbar and Explorer all show one icon.
/// Embedded with `include_bytes!` rather than read at runtime, so a moved or
/// missing file cannot leave the window iconless.
#[cfg(not(target_arch = "wasm32"))]
fn load_icon() -> egui::IconData {
    const ICON_PNG: &[u8] = include_bytes!("../icon/icon256.png");

    // A decode failure must never stop the application from starting; a 1x1
    // transparent icon just means the platform falls back to its default.
    match image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.into_rgba8();
            let (width, height) = img.dimensions();
            egui::IconData {
                rgba: img.into_raw(),
                width,
                height,
            }
        }
        Err(e) => {
            log::warn!("could not decode the window icon: {e}");
            egui::IconData {
                rgba: vec![0; 4],
                width: 1,
                height: 1,
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
