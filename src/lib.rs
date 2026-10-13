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
pub mod bandplan;
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
pub mod web {
    use wasm_bindgen::prelude::*;

    /// Hand `bytes` to the browser as a file the user downloads.
    ///
    /// The nearest thing a tab has to "Save as...". There is no filesystem to
    /// write to and no path to pick, so the bytes become an in-memory Blob,
    /// the Blob gets a temporary object URL, and a detached `<a download>` is
    /// clicked to make the browser offer it. Where the file ends up, and under
    /// what name, is then between the browser and its user -- `name` is only
    /// a suggestion.
    ///
    /// The object URL is revoked a moment later rather than immediately.
    /// Skipping the revoke would pin every exported image in memory for the
    /// life of the page -- tens of megabytes each for a full waterfall -- but
    /// doing it in the same synchronous turn as the click is not safe either:
    /// `click()` dispatches synchronously while the download is a queued task,
    /// and the blob URL is resolved when that task creates its request. Chrome
    /// and Firefox happen to resolve early enough; WebKit has historically
    /// cancelled the download. One turn of the event loop costs nothing and
    /// removes the question.
    pub fn download(name: &str, mime: &str, bytes: &[u8]) -> Result<(), String> {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or("no document")?;

        // `Uint8Array::from` copies into the JS heap; the Blob then owns that
        // copy, so the Rust buffer can go out of scope the moment we return.
        let array = js_sys::Array::new();
        array.push(&js_sys::Uint8Array::from(bytes).into());

        let options = web_sys::BlobPropertyBag::new();
        options.set_type(mime);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&array, &options)
            .map_err(|e| format!("could not build the blob: {e:?}"))?;

        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|e| format!("could not create an object URL: {e:?}"))?;

        let anchor = document
            .create_element("a")
            .map_err(|e| format!("could not create the link: {e:?}"))?
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| "not an anchor".to_owned())?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();

        let revoke = Closure::once_into_js(move || {
            let _ = web_sys::Url::revoke_object_url(&url);
        });
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                revoke.unchecked_ref(),
                // Long enough for the queued download task to have run, short
                // enough that nothing noticeable is held.
                1_000,
            );
        }
        Ok(())
    }

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

/// Entry point `cargo-apk2` / `cargo-ndk` wires up through the
/// `android-activity` glue inside winit.
///
/// The whole application lives in this library precisely so that Android and
/// the web can own the event loop; `main.rs` is only used for desktop builds.
///
/// `cargo check --target aarch64-linux-android` compiles this without an NDK
/// (nothing is linked), which is what CI does; producing an .apk needs the SDK
/// and NDK, and `BUILDING.md` has the toolchain setup.
#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: android_activity::AndroidApp) {
    // From winit directly: eframe re-exports egui, epaint, egui_wgpu and wgpu,
    // but neither winit nor egui-winit, so there is no `eframe::egui_winit`
    // path to reach this through.
    use winit::platform::android::EventLoopBuilderExtAndroid;

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
