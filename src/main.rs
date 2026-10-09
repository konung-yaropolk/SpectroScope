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

/// The window/taskbar icon, drawn at startup rather than shipped as a file.
///
/// It is a miniature waterfall: a Turbo-coloured spectral ridge, which is both
/// recognisably this application and free of any image-decoding dependency.
#[cfg(not(target_arch = "wasm32"))]
fn load_icon() -> egui::IconData {
    const N: u32 = 64;
    let lut = &spectroscope::colormap::LUTS[4]; // Turbo
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);

    for y in 0..N {
        for x in 0..N {
            // Two peaks plus a noise floor, smeared slightly over time (y).
            let fx = x as f32 / N as f32;
            let drift = (y as f32 / N as f32) * 0.06;
            let peak = |c: f32, w: f32| (-((fx - c) / w).powi(2)).exp();
            let v = 0.12 + 0.80 * peak(0.33 + drift, 0.055) + 0.55 * peak(0.68 - drift, 0.075);

            let i = (v.clamp(0.0, 1.0) * 255.0) as usize;
            let [r, g, b] = lut[i];
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }

    egui::IconData {
        rgba,
        width: N,
        height: N,
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
