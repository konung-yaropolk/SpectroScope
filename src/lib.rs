//! SpectroScope -- a spectrum analyzer for multiple SDR platforms.
//!
//! A Rust/egui rewrite of [QSpectrumAnalyzer], rendering through wgpu so the
//! same code runs on Windows, Linux, macOS, Android and the web.
//!
//! The crate is also a library: [`sources::SpectrumSource`] is the extension
//! point for new SDR hardware, and the DSP and data layers are usable on their
//! own.
//!
//! [QSpectrumAnalyzer]: https://github.com/xmikos/qspectrumanalyzer

// Sweep parameters arrive as `f64` straight from spin boxes and from backend
// output, so NaN is a real input, not a theoretical one. `!(x > 0.0)` is the
// idiom that rejects it: the clippy-preferred `x <= 0.0` is *false* for NaN and
// would wave it through into the hop arithmetic. The lint is asking for the
// wrong thing here, and the guards it points at are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]
// `let mut c = Config::default(); c.field = x;` reads better than restating a
// twenty-field struct literal, which is what most of these would become.
#![allow(clippy::field_reassign_with_default)]

pub mod app;
pub mod colormap;
pub mod config;
pub mod data;
pub mod dsp;
pub mod sources;
pub mod ui;
pub mod util;

pub use app::SpectroScopeApp;

/// Shown in the window title and the About window.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = "SpectroScope";

/// The eframe id under which window geometry and settings are stored.
pub const APP_ID: &str = "spectroscope";

// ---------------------------------------------------------------------------
// Web entry point
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// Mount the application on the canvas with the given element id.
    ///
    /// Called from `index.html`; returns a `Promise` that rejects if the canvas
    /// cannot be found or wgpu cannot be initialised.
    #[wasm_bindgen]
    pub async fn start(canvas_id: String) -> Result<(), JsValue> {
        console_error_panic_hook::set_once();

        let document = web_sys::window()
            .ok_or_else(|| JsValue::from_str("no window"))?
            .document()
            .ok_or_else(|| JsValue::from_str("no document"))?;

        let canvas = document
            .get_element_by_id(&canvas_id)
            .ok_or_else(|| JsValue::from_str("canvas element not found"))?
            .dyn_into::<web_sys::HtmlCanvasElement>()?;

        let options = eframe::WebOptions::default();

        eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| Ok(Box::new(crate::SpectroScopeApp::new(cc)))),
            )
            .await
            .map_err(|e| JsValue::from_str(&format!("{e:?}")))
    }
}

// ---------------------------------------------------------------------------
// Android entry point
// ---------------------------------------------------------------------------

/// Entry point `cargo-apk` / `cargo-ndk` wires up through the
/// `android-activity` glue inside winit.
///
/// The whole application lives in this library precisely so that Android and
/// the web can own the event loop; `main.rs` is only used for desktop builds.
///
/// Note: this cannot be compiled without the Android NDK, so it is gated off
/// every other target. See `BUILDING.md` for the toolchain setup.
#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: android_activity::AndroidApp) {
    use eframe::egui_winit::winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        event_loop_builder: Some(Box::new(move |builder| {
            builder.with_android_app(app);
        })),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(SpectroScopeApp::new(cc)))),
    ) {
        log::error!("SpectroScope exited with an error: {e}");
    }
}
