//! The main window: menu bar, control panels, the spectrum/waterfall split and
//! the acquisition lifecycle.
//!
//! Replaces `QSpectrumAnalyzerMainWindow` from `__main__.py`. Qt dock widgets
//! have no egui equivalent, so the four docks (*Controls*, *Frequency*,
//! *Settings*, *Levels*) became collapsible sections of one resizable side
//! panel, which keeps the same grouping and the same reading order.

use std::collections::VecDeque;

use crossbeam_channel::{Receiver, TryRecvError};

use crate::config::Config;
use crate::data::{Baseline, DataStorage, Recorder, Updated};
use crate::sources::{EventSink, Frame, SourceEvent, SourceSession, SpectrumSource, SweepConfig};
use crate::ui::{Dialogs, LevelsPanel, SpectrumPlot, Waterfall, WaterfallView};
use crate::util::{human_time, now_monotonic};
use crate::{APP_NAME, VERSION};

/// Lines kept in the log pane. Enough to see a backend's startup complaints
/// without letting a chatty backend grow unboundedly.
const LOG_CAPACITY: usize = 500;

/// Cap on frames drained per repaint. A backend that outruns the display must
/// not be able to starve the GUI, but the queue still drains because every
/// drained frame also requests another repaint.
const MAX_FRAMES_PER_REPAINT: usize = 8;

/// Share of the window the controls panel takes on a first run.
const PANEL_WIDTH_FRACTION: f32 = 0.20;

/// What the browser is offered when no name has been typed.
///
/// A name rather than a path: in a tab the user does not choose where it
/// lands, only what it is called, and `.png` is the half of the pair somebody
/// looking at a picture wants.
#[cfg(target_arch = "wasm32")]
const DEFAULT_IMAGE_NAME: &str = "spectroscope-waterfall.png";

/// Narrowest the controls panel may be dragged, in points.
///
/// Not a taste call: it is what the widest row in the panel measures once
/// every combo has given back the width it does not need. The test
/// `the_side_panel_keeps_the_width_it_is_dragged_to` is what holds it true.
const MIN_PANEL_WIDTH: f32 = 190.0;

pub struct SpectroScopeApp {
    cfg: Config,
    storage: DataStorage,

    spectrum: SpectrumPlot,
    waterfall: Waterfall,
    levels: LevelsPanel,
    dialogs: Dialogs,
    show_log: bool,

    source: &'static dyn SpectrumSource,
    session: Option<Box<dyn SourceSession>>,
    events: Option<Receiver<SourceEvent>>,
    running: bool,
    hops: usize,

    /// Geometry the waterfall textures were last built for.
    wf_geometry: (usize, usize),

    log: VecDeque<String>,
    error: Option<String>,

    /// Open recording, if any. Sweeps are appended as they arrive.
    recorder: Option<Recorder>,

    /// Monotonic seconds, for the status bar and the progress bar.
    run_started_at: f64,
    last_frame_at: f64,
    /// Smoothed seconds per sweep. Used for the status bar and for the
    /// waterfall time axis.
    sweep_time: f64,
    /// Sweeps accepted this run, so the first gap can be ignored.
    sweeps_seen: u64,
    /// When the first sweep of the run landed.
    first_frame_at: f64,
}

impl SpectroScopeApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut cfg = cc
            .storage
            .and_then(|s| eframe::get_value::<Config>(s, crate::config::STORAGE_KEY))
            .unwrap_or_default();
        // Before clamping, so a raised default is still subject to the limits.
        cfg.migrate();
        cfg.clamp_to(&cfg.source().info().limits);

        Self::build(&cc.egui_ctx, cfg, cc.wgpu_render_state.clone())
    }

    /// Everything in [`Self::new`] that does not need an eframe context.
    ///
    /// Split out so the layout can be exercised in a plain `egui::Context`,
    /// where there is neither storage nor a GPU.
    fn build(
        egui_ctx: &egui::Context,
        cfg: Config,
        render_state: Option<eframe::egui_wgpu::RenderState>,
    ) -> Self {
        let mut waterfall = Waterfall::new(render_state);
        waterfall.set_colormap(cfg.levels.lut(), cfg.levels.reverse);

        apply_theme(egui_ctx, cfg.dark_mode);

        let source = cfg.source();
        let now = now_monotonic();

        Self {
            storage: DataStorage::new(cfg.waterfall_history_size),
            source,
            cfg,
            spectrum: SpectrumPlot::new(),
            waterfall,
            levels: LevelsPanel::new(),
            dialogs: Dialogs::default(),
            show_log: false,
            session: None,
            events: None,
            running: false,
            hops: 0,
            wf_geometry: (0, 0),
            log: VecDeque::new(),
            error: None,
            recorder: None,
            run_started_at: now,
            last_frame_at: now,
            sweep_time: 0.0,
            sweeps_seen: 0,
            first_frame_at: now,
        }
    }

    // --- acquisition ------------------------------------------------------

    fn start(&mut self, ctx: &egui::Context, single_shot: bool) {
        self.stop();

        self.source = self.cfg.source();
        self.storage
            .set_max_history_size(self.cfg.waterfall_history_size);
        self.storage.reset();
        self.spectrum.clear();
        self.levels.invalidate();
        self.wf_geometry = (0, 0);
        self.error = None;

        // Smoothing and the baseline have to be installed before the first
        // sweep arrives, or the first frame would be processed with the old
        // settings and the history would be inconsistent with the rest.
        self.storage.set_smooth(
            self.cfg.traces.smooth,
            self.cfg.smooth_length,
            self.cfg.smooth_window,
        );
        self.reload_baseline();

        let now = now_monotonic();
        self.run_started_at = now;
        self.last_frame_at = now;
        self.sweep_time = 0.0;
        self.sweeps_seen = 0;
        self.first_frame_at = now;

        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = EventSink::new(tx, Some(ctx.clone()));
        let config: SweepConfig = self.cfg.sweep_config(single_shot);

        match self.source.start(config, sink) {
            Ok(session) => {
                self.session = Some(session);
                self.events = Some(rx);
                self.running = true;
            }
            Err(e) => {
                self.push_log(format!("Failed to start {}: {e}", self.source.info().id));
                self.error = Some(e.to_string());
                self.running = false;
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.stop();
        }
        self.events = None;
        self.running = false;
        // A recording outliving the acquisition that feeds it would just be an
        // open file accruing nothing.
        self.stop_recording();
    }

    /// Begin writing sweeps to the configured file.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_recording(&mut self) {
        self.stop_recording();

        let path = self.cfg.record_file.trim().to_owned();
        if path.is_empty() {
            self.error = Some("Choose a file to record into first.".to_owned());
            return;
        }

        match Recorder::create(&path, self.cfg.record_format) {
            Ok(rec) => {
                self.push_log(format!(
                    "Recording to {path} as {}",
                    self.cfg.record_format.label()
                ));
                self.recorder = Some(rec);
            }
            Err(e) => {
                self.push_log(format!("Cannot record to {path}: {e}"));
                self.error = Some(format!("Cannot record to {path}: {e}"));
            }
        }
    }

    fn stop_recording(&mut self) {
        let Some(rec) = self.recorder.take() else {
            return;
        };
        let (path, sweeps) = (rec.path().display().to_string(), rec.sweeps());
        match rec.finish() {
            Ok(()) => self.push_log(format!("Recorded {sweeps} sweeps to {path}")),
            Err(e) => {
                self.push_log(format!("Error closing {path}: {e}"));
                self.error = Some(format!("Error closing {path}: {e}"));
            }
        }
    }

    /// What the frequency and time axes were when the image was taken.
    ///
    /// The pixels alone do not say what they are of, so this travels with them
    /// into the TIFF metadata and onto the PNG's printed ruler.
    fn image_axes(&self) -> crate::data::image_export::Axes {
        match self.storage.x() {
            Some(x) if x.len() > 1 => crate::data::image_export::Axes {
                start_hz: x[0],
                bin_hz: (x[x.len() - 1] - x[0]) / (x.len() - 1) as f64,
                sweep_s: self.sweep_time,
                newest_unix: self.storage.timestamp(),
                plan: self.cfg.active_band_plan(),
                annotate_tiff: self.cfg.annotate_tiff,
            },
            _ => crate::data::image_export::Axes::default(),
        }
    }

    /// Hand the waterfall to the browser as a download.
    ///
    /// A tab has no path to save to, so the same encoder runs and the bytes go
    /// to the browser instead of to a file. The container comes from the name's
    /// extension, exactly as it does from the chosen path on the desktop, so
    /// the two builds produce the same two kinds of file.
    #[cfg(target_arch = "wasm32")]
    fn download_waterfall(&mut self) {
        use crate::data::image_export::{self, ImageFormat};

        let typed = self.cfg.waterfall_image_file.trim();
        let name = if typed.is_empty() {
            DEFAULT_IMAGE_NAME.to_owned()
        } else {
            typed.to_owned()
        };
        let format = ImageFormat::from_path(std::path::Path::new(&name));

        let axes = self.image_axes();
        let encoded = image_export::encode(
            format,
            self.storage.history(),
            self.cfg.levels.lut(),
            self.cfg.levels.low,
            self.cfg.levels.high,
            axes,
        );
        match encoded {
            Ok(img) => match crate::web::download(&name, format.mime(), &img.bytes) {
                Ok(()) => self.push_log(format!(
                    "Saved waterfall {}x{} as {name} ({} kB)",
                    img.width,
                    img.height,
                    img.bytes.len() / 1024
                )),
                Err(e) => {
                    self.push_log(format!("Could not start the download: {e}"));
                    self.error = Some(format!("Could not start the download: {e}"));
                }
            },
            Err(e) => {
                self.push_log(format!("Could not save waterfall: {e}"));
                self.error = Some(format!("Could not save waterfall: {e}"));
            }
        }
    }

    /// Write the stored history out as an image file.
    #[cfg(not(target_arch = "wasm32"))]
    fn save_waterfall(&mut self, path: std::path::PathBuf) {
        {
            let axes = self.image_axes();

            let result = crate::data::image_export::save(
                &path,
                self.storage.history(),
                self.cfg.levels.lut(),
                self.cfg.levels.low,
                self.cfg.levels.high,
                axes,
            );
            match result {
                Ok((w, h)) => {
                    self.cfg.waterfall_image_file = path.display().to_string();
                    self.push_log(format!("Saved waterfall {w}x{h} to {}", path.display()));
                }
                Err(e) => {
                    self.push_log(format!("Could not save waterfall: {e}"));
                    self.error = Some(format!("Could not save waterfall: {e}"));
                }
            }
        }
    }

    /// Replace what is on screen with the sweeps stored in a recording.
    ///
    /// The point of the recording format is that it is the same one
    /// `soapy_power -F soapy_power_bin` writes and the same one a *Baseline* is
    /// read from, so this is the third reader of it rather than a new format:
    /// the sweeps go through [`DataStorage::update`] exactly as if a backend
    /// had just produced them, which is what makes every downstream feature --
    /// the waterfall, the peak holds, smoothing, the band plan, and saving the
    /// result as PNG or TIFF -- work on a loaded file without knowing it came
    /// from one.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_recording(&mut self, path: std::path::PathBuf) {
        use std::io::BufReader;

        // Loading replaces the axis and the history, which a live sweep is in
        // the middle of writing to.
        self.stop();

        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                let msg = format!("Could not open {}: {e}", path.display());
                self.push_log(msg.clone());
                self.error = Some(msg);
                return;
            }
        };

        let frames = crate::data::soapy_bin::read_frames(&mut BufReader::new(file));
        if frames.is_empty() {
            let msg = format!(
                "No sweeps in {}. It has to be a soapy_power binary recording (.bin).",
                path.display()
            );
            self.push_log(msg.clone());
            self.error = Some(msg);
            return;
        }

        // The recording's own cadence, so the time ruler down the side of a
        // saved image reads in the units the capture was made at rather than
        // in whatever the last live run happened to be doing.
        let span = frames
            .last()
            .zip(frames.first())
            .map(|(l, f)| l.timestamp - f.timestamp)
            .unwrap_or(0.0);
        let sweep_time = if frames.len() > 1 && span.is_finite() && span > 0.0 {
            span / (frames.len() - 1) as f64
        } else {
            0.0
        };

        self.storage.reset();
        self.spectrum.clear();
        self.sweeps_seen = 0;
        self.hops = 0;

        let total = frames.len();
        let mut rejected = 0usize;
        for frame in frames {
            if self.storage.update(frame).is_err() {
                rejected += 1;
            }
        }
        let shown = self.storage.history().len();
        self.sweeps_seen = (total - rejected) as u64;
        self.sweep_time = sweep_time;

        // Nothing was uploaded row by row, so the textures are rebuilt from the
        // whole ring in one go. A geometry of zero can never match the real
        // one, which is what forces that path.
        self.wf_geometry = (0, 0);
        self.sync_waterfall(Updated {
            recalculated: true,
            ..Default::default()
        });
        if self.cfg.traces.persistence {
            let length = self.cfg.persistence_length;
            self.spectrum.refill_persistence(&mut self.storage, length);
        }
        self.spectrum.request_refit();
        self.waterfall.reset_view();

        self.cfg.record_file = path.display().to_string();
        let mut note = format!("Loaded {total} sweeps from {}", path.display());
        if shown < total - rejected {
            // The ring is the display's, not the file's: saying so is what
            // stops "I recorded an hour and only see ten minutes" being a bug
            // report.
            use std::fmt::Write as _;
            let _ = write!(
                note,
                "; showing the newest {shown} (History size in Settings sets the limit)"
            );
        }
        if rejected > 0 {
            use std::fmt::Write as _;
            let _ = write!(note, "; skipped {rejected} with a different bin count");
        }
        self.push_log(note);
    }

    /// Put both views back where a fresh run would have them.
    fn reset_view(&mut self) {
        self.spectrum.request_refit();
        self.waterfall.reset_view();
    }

    fn push_log(&mut self, line: impl Into<String>) {
        let line = line.into();
        log::info!("{line}");
        self.log.push_back(line);
        while self.log.len() > LOG_CAPACITY {
            self.log.pop_front();
        }
    }

    /// Drain the source's events into the data store.
    fn pump_events(&mut self) -> Updated {
        let mut updated = Updated::default();
        let mut frames = 0;

        loop {
            if frames >= MAX_FRAMES_PER_REPAINT {
                break;
            }
            // The receiver is borrowed only long enough to take one event: the
            // handlers below need `&mut self`, so the borrow cannot straddle
            // them.
            let Some(event) = self.events.as_ref().map(|rx| rx.try_recv()) else {
                break;
            };
            match event {
                Ok(SourceEvent::Started { hops }) => {
                    self.hops = hops;
                    self.running = true;
                }
                Ok(SourceEvent::Frame(frame)) => {
                    frames += 1;
                    updated = merge(updated, self.accept_frame(frame));
                }
                Ok(SourceEvent::Log(line)) => self.push_log(line),
                Ok(SourceEvent::Error(line)) => {
                    self.push_log(format!("Error: {line}"));
                    self.error = Some(line);
                }
                Ok(SourceEvent::Stopped) => {
                    self.running = false;
                    // Keep the receiver: a source may emit queued frames just
                    // before Stopped, and dropping it here would discard them.
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = false;
                    self.events = None;
                    break;
                }
            }
        }

        updated
    }

    fn accept_frame(&mut self, frame: Frame) -> Updated {
        let now = now_monotonic();
        self.last_frame_at = now;
        self.sweeps_seen += 1;
        if self.sweeps_seen == 1 {
            self.first_frame_at = now;
        }
        self.sweep_time = average_sweep_time(now, self.first_frame_at, self.sweeps_seen);

        if let Some(rec) = self.recorder.as_mut() {
            if let Err(e) = rec.write(&frame) {
                // A failed write means the capture is no longer being saved,
                // which the user has to be told about rather than discover
                // later from a short file.
                let msg = format!("Recording stopped: {e}");
                self.recorder = None;
                self.push_log(msg.clone());
                self.error = Some(msg);
            }
        }

        match self.storage.update(frame) {
            Ok(updated) => {
                self.sync_waterfall(updated);
                if self.cfg.traces.persistence {
                    let y = self.storage.y().to_vec();
                    self.spectrum
                        .push_persistence(&y, self.cfg.persistence_length);
                }
                updated
            }
            Err(mismatch) => {
                // The backend changed its bin count mid-run, which the derived
                // series cannot absorb. Say so rather than silently dropping
                // sweeps, which is what the Python build did.
                self.push_log(mismatch.to_string());
                Updated::default()
            }
        }
    }

    /// Keep the waterfall's textures in step with the history ring.
    fn sync_waterfall(&mut self, updated: Updated) {
        let history = self.storage.history();
        let geometry = (history.bins(), history.capacity());
        if geometry.0 == 0 {
            return;
        }

        if geometry != self.wf_geometry {
            self.waterfall.reset(geometry.0, geometry.1);
            self.waterfall.upload_history(history);
            self.wf_geometry = geometry;
            return;
        }

        if updated.recalculated {
            self.waterfall.upload_history(history);
            return;
        }

        // The common path: exactly one new row, so exactly one row is uploaded.
        if updated.history {
            if let (Some(index), Some(row)) =
                (history.row_index_from_newest(0), history.row_from_newest(0))
            {
                self.waterfall.push_row(index, row);
            }
        }
    }

    fn reload_baseline(&mut self) {
        let baseline = self.load_baseline_file();
        let updated = self
            .storage
            .set_baseline(self.cfg.traces.subtract_baseline, baseline);
        self.after_recalculation(updated);
    }

    fn load_baseline_file(&mut self) -> Baseline {
        if self.cfg.baseline_file.trim().is_empty() {
            return Baseline::default();
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::BufReader;
            let path = self.cfg.baseline_file.clone();
            match std::fs::File::open(&path) {
                Ok(file) => {
                    let mut reader = BufReader::new(file);
                    let frames = crate::data::soapy_bin::read_frames(&mut reader);
                    match crate::data::soapy_bin::average_frames(&frames) {
                        Some((x, y)) => {
                            self.push_log(format!(
                                "Baseline: averaged {} sweeps of {} bins from {path}",
                                frames.len(),
                                y.len()
                            ));
                            Baseline {
                                x: Some(x),
                                y: Some(y),
                            }
                        }
                        None => {
                            self.push_log(format!("Baseline: no usable sweeps in {path}"));
                            Baseline::default()
                        }
                    }
                }
                Err(e) => {
                    self.push_log(format!("Baseline: cannot open {path}: {e}"));
                    Baseline::default()
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Browsers have no path-addressable filesystem; a baseline would
            // have to arrive through a file input or a drop.
            self.push_log("Baseline files are not available in the browser build.");
            Baseline::default()
        }
    }

    /// Refresh everything that a full recalculation invalidates.
    fn after_recalculation(&mut self, updated: Updated) {
        if !updated.any() {
            return;
        }
        self.spectrum.sync(&self.storage, updated);
        if updated.recalculated || updated.history {
            self.waterfall.upload_history(self.storage.history());
            self.levels.invalidate();
        }
        if self.cfg.traces.persistence {
            let length = self.cfg.persistence_length;
            self.spectrum.refill_persistence(&mut self.storage, length);
        }
    }

    // --- panels -----------------------------------------------------------

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("spectroscope.menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings...").clicked() {
                        self.dialogs.open_settings(&self.cfg);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.close();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.cfg.show_controls_panel, "Control panel");
                    ui.checkbox(&mut self.cfg.show_waterfall, "Waterfall");
                    ui.add_enabled(
                        self.cfg.show_waterfall,
                        egui::Checkbox::new(&mut self.cfg.show_waterfall_ruler, "Frequency ruler"),
                    );
                    ui.checkbox(&mut self.show_log, "Backend log");
                    ui.separator();
                    if ui.checkbox(&mut self.cfg.dark_mode, "Dark theme").changed() {
                        apply_theme(ctx, self.cfg.dark_mode);
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.dialogs.open_about();
                        ui.close();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.running {
                        ui.label(
                            egui::RichText::new("● running")
                                .color(egui::Color32::from_rgb(120, 220, 120)),
                        );
                    }
                    ui.label(egui::RichText::new(self.source.info().label).weak());
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("spectroscope.status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(err) = self.error.clone() {
                    ui.label(
                        egui::RichText::new(format!("⚠ {err}")).color(ui.visuals().error_fg_color),
                    );
                    if ui.small_button("dismiss").clicked() {
                        self.error = None;
                    }
                    ui.separator();
                }

                let mut parts: Vec<String> = Vec::new();
                if self.hops > 0 {
                    parts.push(format!("Frequency hops: {}", self.hops));
                }
                if self.storage.has_data() || self.running {
                    let total = now_monotonic() - self.run_started_at;
                    let fps = if self.sweep_time > 0.0 {
                        1.0 / self.sweep_time
                    } else {
                        0.0
                    };
                    parts.push(format!(
                        "Total time: {} | Sweep time: {:.2} s ({:.2} FPS)",
                        human_time(total),
                        self.sweep_time,
                        fps
                    ));
                }
                if self.storage.has_data() {
                    parts.push(format!("{} bins", self.storage.bins()));
                }
                ui.label(parts.join(" | "));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.running {
                        ui.add(self.progress_bar());
                    }
                });
            });
        });
    }

    /// Progress towards the next expected sweep.
    ///
    /// Scaled by how long a sweep is *measured* to take, not by the configured
    /// interval. The interval is only a request for how long to dwell: every
    /// hopping backend spends retuning and settling time on top of it, so an
    /// `rtl_tcp` sweep of 34 hops runs several times the interval. Scaling the
    /// bar by the interval therefore drove it past full within the first
    /// second of every sweep, where the overdue branch pinned it at 1.0 with
    /// the marching animation -- a bar that looked stuck because it was,
    /// permanently, at the end.
    ///
    /// Until a second sweep has landed there is nothing measured to scale by,
    /// and the animation is then the honest answer rather than a wrong number.
    fn progress_bar(&self) -> egui::ProgressBar {
        let elapsed = now_monotonic() - self.last_frame_at;
        match sweep_progress(elapsed, self.sweep_time, self.cfg.interval) {
            Some((fraction, remaining)) => egui::ProgressBar::new(fraction as f32)
                .desired_width(250.0)
                .text(format!("{remaining:.1} s")),
            None => egui::ProgressBar::new(1.0)
                .desired_width(250.0)
                .animate(true)
                .text(""),
        }
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        // A fifth of the window rather than a fixed number of points. A fixed
        // default is wrong at both ends: a sliver on a 4K display, half the
        // spectrum on a small laptop. Only the *default* -- egui persists
        // whatever width it is dragged to, so this is what a first run gets.
        //
        // The upper bound follows the window for the same reason, and the
        // lower one does not: 190 is what the contents actually need, and no
        // window being narrow makes them need less.
        let window = ctx.screen_rect().width();
        let (min, max) = (MIN_PANEL_WIDTH, (window * 0.5).max(MIN_PANEL_WIDTH));
        egui::SidePanel::right("spectroscope.controls")
            .resizable(true)
            .default_width((window * PANEL_WIDTH_FRACTION).clamp(min, max))
            .width_range(min..=max)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.controls_section(ui, ctx);
                    ui.add_space(4.0);
                    self.frequency_section(ui);
                    ui.add_space(4.0);
                    self.settings_section(ui);
                    ui.add_space(4.0);
                    self.recording_section(ui);
                    ui.add_space(4.0);
                    self.levels_section(ui);
                });
            });
    }

    fn controls_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("Controls")
            .default_open(true)
            .show(ui, |ui| {
                // Justified, so the buttons stretch to the panel without
                // reporting that width as a minimum they need.
                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| {
                        // One button that reports the state it will leave you in,
                        // rather than two of which one is always dead.
                        let label = if self.running { "Stop" } else { "Start" };
                        if ui
                            .add(egui::Button::new(label).min_size(egui::vec2(0.0, 28.0)))
                            .clicked()
                        {
                            if self.running {
                                self.stop();
                            } else {
                                self.start(ctx, false);
                            }
                        }
                        if ui
                            .add_enabled(
                                !self.running,
                                egui::Button::new("Single shot").min_size(egui::vec2(0.0, 24.0)),
                            )
                            .clicked()
                        {
                            self.start(ctx, true);
                        }
                        if ui
                            .add(egui::Button::new("Reset view").min_size(egui::vec2(0.0, 24.0)))
                            .on_hover_text(
                                "Fit the spectrum to the sweep and return the waterfall \
                         to the newest sweep at its default scale",
                            )
                            .clicked()
                        {
                            self.reset_view();
                        }
                    },
                );
            });
    }

    fn frequency_section(&mut self, ui: &mut egui::Ui) {
        let limits = self.source.info().limits;
        let lnb_mhz = self.cfg.lnb_lo / 1e6;
        let (start_min, start_max) = self.cfg.start_freq_bounds(&limits, lnb_mhz);
        let (stop_min, stop_max) = self.cfg.stop_freq_bounds(&limits, lnb_mhz);

        egui::CollapsingHeader::new("Frequency")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("freq.grid")
                    .num_columns(2)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Start:");
                        ui.add(
                            egui::DragValue::new(&mut self.cfg.start_freq)
                                .suffix(" MHz")
                                .speed(0.1)
                                .range(start_min..=start_max)
                                .max_decimals(3),
                        );
                        ui.end_row();

                        ui.label("Stop:");
                        ui.add(
                            egui::DragValue::new(&mut self.cfg.stop_freq)
                                .suffix(" MHz")
                                .speed(0.1)
                                .range(stop_min..=stop_max)
                                .max_decimals(3),
                        );
                        ui.end_row();

                        ui.label("Bin size:");
                        ui.add(
                            egui::DragValue::new(&mut self.cfg.bin_size)
                                .suffix(" kHz")
                                .speed(0.1)
                                .range(limits.bin_size.min..=limits.bin_size.max)
                                .max_decimals(3),
                        );
                        ui.end_row();
                    });

                // A backwards range would make the sweep empty, so keep the two
                // fields ordered the way the Qt spin-box minimums did.
                if self.cfg.stop_freq < self.cfg.start_freq {
                    ui.label(
                        egui::RichText::new("Stop must be above start")
                            .small()
                            .color(ui.visuals().warn_fg_color),
                    );
                }
                let bins = self.cfg.sweep_config(false).bin_count();
                ui.label(egui::RichText::new(format!("{bins} bins")).small().weak());
            });
    }

    fn settings_section(&mut self, ui: &mut egui::Ui) {
        let limits = self.source.info().limits;
        let mut smoothing_dirty = false;
        let mut persistence_dirty = false;
        let mut baseline_dirty = false;

        egui::CollapsingHeader::new("Settings")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("settings.inline")
                    .num_columns(2)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Interval [s]:");
                        ui.add(
                            egui::DragValue::new(&mut self.cfg.interval)
                                .speed(0.05)
                                .range(limits.interval.min..=limits.interval.max)
                                .max_decimals(3),
                        );
                        ui.end_row();

                        ui.label("Gain [dB]:");
                        // -1 is the backends' "let the device decide" sentinel,
                        // which the Qt build surfaced as the special value "auto".
                        if limits.gain_choices.is_empty() {
                            let mut gain = self.cfg.gain;
                            let response = ui.add(
                                egui::DragValue::new(&mut gain)
                                    .speed(0.5)
                                    .range(limits.gain.min..=limits.gain.max)
                                    .max_decimals(1)
                                    .custom_formatter(|v, _| {
                                        if v < 0.0 {
                                            "auto".to_owned()
                                        } else {
                                            format!("{v:.1}")
                                        }
                                    })
                                    .custom_parser(|s| {
                                        if s.trim().eq_ignore_ascii_case("auto") {
                                            Some(-1.0)
                                        } else {
                                            s.trim().parse().ok()
                                        }
                                    }),
                            );
                            if response.changed() {
                                self.cfg.gain = gain;
                            }
                        } else {
                            // The tuner snaps anything else to one of these
                            // anyway, so dragging between them only ever moved
                            // the label.
                            egui::ComboBox::from_id_salt("spectroscope.gain")
                                .selected_text(gain_label(self.cfg.gain))
                                .show_ui(ui, |ui| {
                                    if limits.gain.min < 0.0 {
                                        ui.selectable_value(
                                            &mut self.cfg.gain,
                                            -1.0,
                                            gain_label(-1.0),
                                        );
                                    }
                                    for gain in limits.gain_choices {
                                        ui.selectable_value(
                                            &mut self.cfg.gain,
                                            *gain,
                                            gain_label(*gain),
                                        );
                                    }
                                });
                        }
                        ui.end_row();

                        if !limits.ppm.is_fixed() {
                            ui.label("Corr. [ppm]:");
                            ui.add(
                                egui::DragValue::new(&mut self.cfg.ppm)
                                    .speed(1.0)
                                    .range(limits.ppm.min..=limits.ppm.max),
                            );
                            ui.end_row();
                        }

                        if !limits.crop.is_fixed() {
                            ui.label("Crop [%]:");
                            ui.add(
                                egui::DragValue::new(&mut self.cfg.crop)
                                    .speed(1.0)
                                    .range(limits.crop.min..=limits.crop.max),
                            );
                            ui.end_row();
                        }
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cfg.traces.main_curve, "Main curve");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Colors...").clicked() {
                            self.dialogs.open_colors(&self.cfg);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cfg.traces.peak_hold_max, "Max. hold");
                    ui.checkbox(&mut self.cfg.traces.peak_hold_min, "Min. hold");
                });
                ui.checkbox(&mut self.cfg.traces.average, "Average");

                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut self.cfg.traces.smooth, "Smoothing")
                        .changed()
                    {
                        smoothing_dirty = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("...").clicked() {
                            self.dialogs.open_smoothing(&self.cfg);
                        }
                    });
                });

                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut self.cfg.traces.persistence, "Persistence")
                        .changed()
                    {
                        persistence_dirty = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("...").clicked() {
                            self.dialogs.open_persistence(&self.cfg);
                        }
                    });
                });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.cfg.traces.baseline, "Baseline");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("...").clicked() {
                            self.dialogs.open_baseline(&self.cfg);
                        }
                    });
                });
                if ui
                    .checkbox(&mut self.cfg.traces.subtract_baseline, "Subtract baseline")
                    .changed()
                {
                    baseline_dirty = true;
                }

                ui.separator();
                ui.checkbox(&mut self.cfg.show_band_plan, "Band plan")
                    .on_hover_text("Print allocations above the waterfall ruler");
                ui.add_enabled_ui(self.cfg.show_band_plan, |ui| {
                    let current = crate::bandplan::find(&self.cfg.band_plan);
                    egui::ComboBox::from_id_salt("bandplan.region")
                        .selected_text(current.label)
                        .truncate()
                        .width(crate::ui::combo_width(ui, 180.0))
                        .show_ui(ui, |ui| {
                            for plan in crate::bandplan::plans() {
                                // "None" is what the checkbox above is for.
                                if plan.bands.is_empty() {
                                    continue;
                                }
                                let selected = plan.id == self.cfg.band_plan;
                                if ui.selectable_label(selected, plan.label).clicked() {
                                    self.cfg.band_plan = plan.id.to_owned();
                                }
                            }
                        });
                });
            });

        if smoothing_dirty {
            let updated = self.storage.set_smooth(
                self.cfg.traces.smooth,
                self.cfg.smooth_length,
                self.cfg.smooth_window,
            );
            self.after_recalculation(updated);
        }
        if baseline_dirty {
            self.reload_baseline();
        }
        if persistence_dirty && self.cfg.traces.persistence {
            let length = self.cfg.persistence_length;
            self.spectrum.refill_persistence(&mut self.storage, length);
        }
    }

    fn recording_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Recording")
            .default_open(false)
            .show(ui, |ui| {
                #[cfg(target_arch = "wasm32")]
                self.export_controls(ui);

                #[cfg(not(target_arch = "wasm32"))]
                self.recording_controls(ui);
            });
    }

    /// Image export in the browser.
    ///
    /// Only half of the desktop panel survives the move into a tab, and the
    /// halves divide on whether the feature needs somewhere to *append*.
    /// Recording does -- it writes a sweep at a time for hours, so that a
    /// capture cut short is still a readable file, and nothing a page can hold
    /// gives that. Saving the waterfall does not: it is one buffer, encoded
    /// once, and a browser already has a way to hand the user a finished file.
    #[cfg(target_arch = "wasm32")]
    fn export_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(
                "Recording sweeps to a file needs somewhere to append to, so it \
                 is desktop-only. Saving the waterfall is not -- it is encoded \
                 here and handed to the browser as a download.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Save as:");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.cfg.waterfall_image_file)
                        .hint_text(DEFAULT_IMAGE_NAME)
                        .desired_width(ui.available_width().max(24.0)),
                );
            });
        })
        .response
        .on_hover_text(
            "The extension picks the container, the same as the save dialog does \
             on the desktop: .png for a picture with the ruler drawn on, .tif for \
             the raw 32-bit values ImageJ measures. The browser decides where it \
             lands.",
        );

        ui.checkbox(&mut self.cfg.annotate_tiff, "Label the TIFF too")
            .on_hover_text(
                "Draw the ruler and band plan into a saved TIFF as well. They go \
                 in at the display window's own extremes so the colour map paints \
                 them -- but they do overwrite those pixels, which the PNG does not.",
            );

        let have_history = !self.storage.history().is_empty();
        if ui
            .add_enabled(
                have_history,
                egui::Button::new("Save waterfall").min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text("Encode the stored history and download it, one pixel per bin")
            .clicked()
        {
            self.download_waterfall();
        }

        if have_history {
            let h = self.storage.history();
            ui.label(
                egui::RichText::new(format!("{} x {} px", h.bins(), h.len()))
                    .small()
                    .weak(),
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recording_controls(&mut self, ui: &mut egui::Ui) {
        let recording = self.recorder.is_some();

        ui.horizontal(|ui| {
            ui.label("File:");
            // Right-to-left so the browse button reserves its own space and
            // the field takes exactly what is left, instead of the row
            // overflowing and dragging the panel wider every frame.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!recording, egui::Button::new("..."))
                    .clicked()
                {
                    let ext = self.cfg.record_format.extension();
                    if let Some(p) = save_file_dialog(
                        "Record sweeps to",
                        &self.cfg.record_file,
                        &[("Sweep data", &[ext])],
                        ext,
                    ) {
                        self.cfg.record_file = p;
                    }
                }
                ui.add_enabled(
                    !recording,
                    egui::TextEdit::singleline(&mut self.cfg.record_file)
                        .desired_width(ui.available_width().max(24.0)),
                );
            });
        });

        ui.horizontal(|ui| {
            ui.label("Format:");
            egui::ComboBox::from_id_salt("record.format")
                .selected_text(self.cfg.record_format.label())
                .truncate()
                .width(crate::ui::combo_width(ui, 190.0))
                .show_ui(ui, |ui| {
                    for f in crate::data::RecordFormat::ALL {
                        ui.add_enabled_ui(!recording, |ui| {
                            ui.selectable_value(&mut self.cfg.record_format, f, f.label());
                        });
                    }
                });
        });

        let label = if recording {
            "Stop recording"
        } else {
            "Record"
        };
        if ui
            .add(egui::Button::new(label).min_size(egui::vec2(0.0, 24.0)))
            .on_hover_text(
                "Sweeps are appended and flushed one at a time, so an interrupted \
                         recording still opens",
            )
            .clicked()
        {
            if recording {
                self.stop_recording();
            } else {
                self.start_recording();
            }
        }

        if ui
            .add_enabled(
                !recording,
                egui::Button::new("Load recording...").min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text(
                "Open a .bin recording and show it here, so it can be examined, \
                 smoothed and saved as a PNG or TIFF. This stops the acquisition \
                 and replaces what is on screen.",
            )
            .clicked()
        {
            if let Some(p) = open_file_dialog(
                "Load recorded sweeps",
                &self.cfg.record_file,
                &[("soapy_power binary", &["bin"])],
            ) {
                self.load_recording(std::path::PathBuf::from(p));
            }
        }

        match self.recorder.as_ref() {
            Some(rec) => {
                ui.label(
                    egui::RichText::new(format!(
                        "● {} sweeps, {}",
                        rec.sweeps(),
                        human_bytes(rec.bytes())
                    ))
                    .small()
                    .color(egui::Color32::from_rgb(230, 120, 120)),
                );
            }
            None if !self.running => {
                ui.label(
                    egui::RichText::new("Recording follows the acquisition.")
                        .small()
                        .weak(),
                );
            }
            None => {}
        }

        ui.separator();

        let have_history = !self.storage.history().is_empty();
        if ui
            .add_enabled(
                have_history,
                egui::Button::new("Save waterfall...").min_size(egui::vec2(0.0, 24.0)),
            )
            .on_hover_text("Write the stored history as a PNG or TIFF, one pixel per bin")
            .clicked()
        {
            if let Some(p) = save_file_dialog(
                "Save waterfall image",
                &self.cfg.waterfall_image_file,
                &[("PNG image", &["png"]), ("TIFF image", &["tif", "tiff"])],
                "png",
            ) {
                self.save_waterfall(std::path::PathBuf::from(p));
            }
        }
        ui.checkbox(&mut self.cfg.annotate_tiff, "Label the TIFF too")
            .on_hover_text(
                "Draw the ruler and band plan into a saved TIFF as well. They go \
                 in at the display window's own extremes so the colour map paints \
                 them -- but they do overwrite those pixels, which the PNG does not.",
            );

        if have_history {
            let h = self.storage.history();
            ui.label(
                egui::RichText::new(format!("{} x {} px", h.bins(), h.len()))
                    .small()
                    .weak(),
            );
        }
    }

    fn levels_section(&mut self, ui: &mut egui::Ui) {
        let mut outcome = None;
        egui::CollapsingHeader::new("Levels")
            .default_open(true)
            .show(ui, |ui| {
                outcome = Some(self.levels.ui(ui, &mut self.cfg.levels, &self.storage));
            });

        if outcome.is_some_and(|o| o.colormap_changed) {
            self.waterfall
                .set_colormap(self.cfg.levels.lut(), self.cfg.levels.reverse);
        }
    }

    fn central(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let total = ui.available_height();
            if !self.cfg.show_waterfall {
                self.spectrum
                    .ui(ui, &self.cfg, (total - READOUT_HEIGHT).max(1.0));
                return;
            }

            let handle = 7.0;
            let spectrum_height = split_height(total, handle, self.cfg.plot_split);

            self.spectrum
                .ui(ui, &self.cfg, (spectrum_height - READOUT_HEIGHT).max(1.0));

            // A draggable separator, standing in for the Qt QSplitter.
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), handle),
                egui::Sense::drag(),
            );
            if response.dragged() {
                self.cfg.plot_split =
                    (self.cfg.plot_split + response.drag_delta().y / total).clamp(0.1, 0.9);
            }
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            let color = if response.hovered() || response.dragged() {
                ui.visuals().widgets.hovered.bg_fill
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke.color
            };
            ui.painter()
                .rect_filled(rect.shrink2(egui::vec2(0.0, 2.5)), 1.0, color);

            let data_x = match self.storage.x() {
                Some(x) if x.len() > 1 => (x[0], x[x.len() - 1]),
                _ => (0.0, 1.0),
            };
            let view_x = if self.spectrum.view_x.1 > self.spectrum.view_x.0 {
                self.spectrum.view_x
            } else {
                data_x
            };

            // Reserve exactly the strips the spectrum plot spends on its axis
            // labels, so the two images line up column for column.
            let area = ui.max_rect();
            let (gutter_left, gutter_right) = match self.spectrum.plot_rect {
                Some(plot) => (
                    (plot.left() - area.left()).max(0.0),
                    (area.right() - plot.right()).max(0.0),
                ),
                None => (0.0, 0.0),
            };

            let history = self.storage.history();
            let view = WaterfallView {
                low: self.cfg.levels.low,
                high: self.cfg.levels.high,
                data_x,
                view_x,
                row_interval: if self.sweep_time > 0.0 {
                    self.sweep_time
                } else {
                    0.0
                },
                counter: history.counter(),
                gutter_left,
                gutter_right,
                show_ruler: self.cfg.show_waterfall_ruler,
                plan: self.cfg.active_band_plan(),
            };
            let (head, rows) = (history.head(), history.len());
            let wf = self.waterfall.ui(ui, head, rows, &view);

            // The waterfall does not own the frequency axis, so a zoom or pan
            // there is applied to the plot and picked up here next frame.
            if let Some((x0, x1)) = wf.requested_view_x {
                self.spectrum.set_view_x(x0, x1);
            }
        });
    }

    fn log_window(&mut self, ctx: &egui::Context) {
        if !self.show_log {
            return;
        }
        let mut open = true;
        egui::Window::new("Backend log")
            .open(&mut open)
            .default_size([760.0, 320.0])
            .resizable(true)
            .show(ctx, |ui| {
                if ui.button("Clear").clicked() {
                    self.log.clear();
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.add(
                                egui::Label::new(egui::RichText::new(line).monospace().small())
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                    });
            });
        self.show_log = open;
    }
}

impl eframe::App for SpectroScopeApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, crate::config::STORAGE_KEY, &self.cfg);
    }

    fn on_exit(&mut self) {
        // The acquisition owns a child process or a socket; leaving it running
        // after the window closes would strand it.
        self.stop();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let backend_before = self.cfg.backend.clone();
        let updated = self.pump_events();
        // Also when only the view moved: the curves are decimated against it.
        if updated.any() || self.spectrum.needs_resync() {
            self.spectrum.sync(&self.storage, updated);
        }

        self.menu_bar(ctx);
        self.status_bar(ctx);
        if self.cfg.show_controls_panel {
            self.side_panel(ctx);
        }
        self.central(ctx);
        self.log_window(ctx);

        let outcome = self.dialogs.ui(ctx, &mut self.cfg);
        if outcome.backend_changed || self.cfg.backend != backend_before {
            let was_running = self.running;
            self.stop();
            self.source = self.cfg.source();
            self.cfg.clamp_to(&self.source.info().limits);
            if was_running {
                self.start(ctx, false);
            }
        }
        if outcome.history_size_changed {
            self.storage
                .set_max_history_size(self.cfg.waterfall_history_size);
            self.wf_geometry = (0, 0);
            self.levels.invalidate();
        }
        if outcome.smoothing_changed {
            let u = self.storage.set_smooth(
                self.cfg.traces.smooth,
                self.cfg.smooth_length,
                self.cfg.smooth_window,
            );
            self.after_recalculation(u);
        }
        if outcome.baseline_changed {
            self.reload_baseline();
        }
        if outcome.persistence_changed && self.cfg.traces.persistence {
            let length = self.cfg.persistence_length;
            self.spectrum.refill_persistence(&mut self.storage, length);
        }

        // While a sweep is in flight the status bar and the progress bar are
        // time-dependent, so repaint steadily; otherwise sleep until the next
        // event wakes us through the EventSink.
        if self.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// A tuner gain as the drop-down shows it, with the negative sentinel spelled
/// out the way the Qt build spelled it.
///
/// Every target: the gain drop-down is in the side panel, which the browser
/// build draws too.
fn gain_label(db: f64) -> String {
    if db < 0.0 {
        "auto".to_owned()
    } else {
        format!("{db:.1} dB")
    }
}

/// `1.2 MB`, for the recording readout.
#[cfg(not(target_arch = "wasm32"))]
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "kB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Choose an existing file, starting wherever `current` points.
///
/// The counterpart to [`save_file_dialog`], and gated the same way; separate
/// because `rfd` has no one call that both saves and opens, and because an open
/// dialog must not pre-fill a file name that does not exist yet.
#[cfg(not(target_arch = "wasm32"))]
fn open_file_dialog(title: &str, current: &str, filters: &[(&str, &[&str])]) -> Option<String> {
    #[cfg(target_os = "android")]
    {
        let _ = (title, current, filters);
        None
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        for (name, exts) in filters {
            dialog = dialog.add_filter(*name, exts);
        }
        let current = std::path::Path::new(current.trim());
        if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
    }
}

/// Native save dialog.
///
/// Android is a `not(wasm32)` target but has no dialog: `rfd` has no backend
/// there, and the platform's own chooser hands back a `content://` URI rather
/// than a path anything here could open. Recording and image export stay
/// reachable on Android by typing a path, which is why the signature does not
/// change -- only the body.
#[cfg(not(target_arch = "wasm32"))]
fn save_file_dialog(
    title: &str,
    current: &str,
    filters: &[(&str, &[&str])],
    default_ext: &str,
) -> Option<String> {
    #[cfg(target_os = "android")]
    {
        let _ = (title, current, filters, default_ext);
        None
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        for (name, exts) in filters {
            dialog = dialog.add_filter(*name, exts);
        }

        let current = std::path::Path::new(current.trim());
        if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        match current.file_name().and_then(|n| n.to_str()) {
            Some(name) => dialog = dialog.set_file_name(name),
            None => dialog = dialog.set_file_name(format!("spectroscope.{default_ext}")),
        }

        dialog.save_file().map(|p| p.to_string_lossy().into_owned())
    }
}

/// Seconds per sweep, averaged over the whole run.
///
/// Deliberately not the gap between the last two sweeps, and not an
/// exponential average of it either. The raw gap is far too jumpy to label a
/// time axis with: the first one measures start-up rather than a sweep, and a
/// browser throttles background timers so hard that a tab returning to the
/// foreground delivers a burst of sweeps microseconds apart -- enough to drag
/// an exponential average down by a factor of five before it recovers. Dividing
/// elapsed time by sweeps counted is immune to how they were delivered, and
/// since changing the interval restarts the run there is no rate change to
/// track within one.
/// How far through the current sweep we are, and how much of it is left.
///
/// `None` means "no idea": either nothing has been measured yet, or the sweep
/// is so overdue that a fraction would be a guess dressed up as a reading. The
/// caller animates the bar in that case.
///
/// `sweep_time` wins over `interval` whenever one has been measured, which is
/// the whole point. The interval is what the user asked each sweep to dwell
/// for; a hopping backend then spends retuning and settling time on top of it,
/// so the two are not the same quantity and scaling by the interval left the
/// bar jammed at full for the whole of every sweep.
fn sweep_progress(elapsed: f64, sweep_time: f64, interval: f64) -> Option<(f64, f64)> {
    let expected = if sweep_time > 0.0 {
        sweep_time
    } else {
        interval
    };
    if !expected.is_finite() || expected <= 0.0 || !elapsed.is_finite() {
        return None;
    }
    if elapsed > expected * 2.0 {
        return None;
    }
    let fraction = (elapsed / expected).clamp(0.0, 1.0);
    Some((fraction, (expected - elapsed).max(0.0)))
}

fn average_sweep_time(now: f64, first_frame_at: f64, sweeps_seen: u64) -> f64 {
    if sweeps_seen < 2 {
        return 0.0;
    }
    let elapsed = now - first_frame_at;
    if !elapsed.is_finite() || elapsed <= 0.0 {
        return 0.0;
    }
    elapsed / (sweeps_seen - 1) as f64
}

/// Height the crosshair readout line above the plot takes.
const READOUT_HEIGHT: f32 = 24.0;

/// Smallest usable height for either pane of the split.
const MIN_PANE_HEIGHT: f32 = 48.0;

/// Height of the spectrum pane for a given split fraction.
///
/// `f32::clamp` panics when `min > max`, which is exactly what a naive
/// `clamp(MIN, total - MIN)` does as soon as the window is shorter than two
/// panes -- briefly true while a web canvas is still sizing itself, and it
/// aborted the whole application. Below that the space is simply halved.
fn split_height(total: f32, handle: f32, fraction: f32) -> f32 {
    let usable = (total - handle).max(0.0);
    if usable < MIN_PANE_HEIGHT * 2.0 {
        return usable * 0.5;
    }
    (total * fraction).clamp(MIN_PANE_HEIGHT, usable - MIN_PANE_HEIGHT)
}

fn merge(a: Updated, b: Updated) -> Updated {
    Updated {
        data: a.data || b.data,
        history: a.history || b.history,
        average: a.average || b.average,
        peak_hold_max: a.peak_hold_max || b.peak_hold_max,
        peak_hold_min: a.peak_hold_min || b.peak_hold_min,
        baseline: a.baseline || b.baseline,
        recalculated: a.recalculated || b.recalculated,
    }
}

/// Pin the theme.
///
/// `set_visuals` is not enough: eframe re-applies the system theme whenever the
/// OS reports one, so a dark preference silently reverted to light on the next
/// frame. Setting the preference is what actually sticks.
fn apply_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
}

/// Window title including the version, matching the Qt build's.
pub fn window_title() -> String {
    format!("{APP_NAME} {VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recording written out and read back has to arrive on screen: the
    /// spectrum, the history ring behind the waterfall, and therefore anything
    /// saved as a PNG or TIFF from it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_recording_loads_back_into_the_view() {
        use crate::data::{RecordFormat, Recorder};
        use std::sync::Arc;

        let mut path = std::env::temp_dir();
        path.push(format!("spectroscope-load-{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let bins = 64usize;
        let x: Arc<Vec<f64>> = Arc::new(
            (0..bins)
                .map(|i| 100e6 + i as f64 * 10e3)
                .collect::<Vec<f64>>(),
        );
        let sweeps = 7usize;
        {
            let mut rec = Recorder::create(&path, RecordFormat::SoapyPowerBin)
                .expect("a temp file can be created");
            for s in 0..sweeps {
                let y: Vec<f32> = (0..bins).map(|i| -90.0 + (i + s) as f32 * 0.25).collect();
                rec.write(&crate::sources::Frame {
                    timestamp: 1_700_000_000.0 + s as f64 * 0.5,
                    x: Arc::clone(&x),
                    y,
                })
                .expect("the write succeeds");
            }
            rec.finish().expect("the file closes");
        }

        let ctx = egui::Context::default();
        let mut app = SpectroScopeApp::build(&ctx, Config::default(), None);
        assert!(!app.storage.has_data(), "nothing is loaded yet");

        app.load_recording(path.clone());

        assert!(app.error.is_none(), "unexpected error: {:?}", app.error);
        assert!(app.storage.has_data(), "the sweeps did not arrive");
        assert_eq!(app.storage.bins(), bins);
        assert_eq!(app.storage.history().len(), sweeps);
        assert_eq!(app.sweeps_seen, sweeps as u64);
        // The newest sweep is the one on the curve.
        let last: Vec<f32> = (0..bins)
            .map(|i| -90.0 + (i + sweeps - 1) as f32 * 0.25)
            .collect();
        assert_eq!(app.storage.y(), last.as_slice());
        // Half a second between sweeps in the file, so the time ruler of a
        // saved image reads the recording's own cadence.
        assert!(
            (app.sweep_time - 0.5).abs() < 1e-6,
            "sweep time {}",
            app.sweep_time
        );
        // And the axis survived, which is what the image's frequency ruler and
        // the band plan are drawn against.
        let axes = app.image_axes();
        assert!((axes.start_hz - 100e6).abs() < 1.0, "{}", axes.start_hz);
        assert!((axes.bin_hz - 10e3).abs() < 1.0, "{}", axes.bin_hz);

        let _ = std::fs::remove_file(&path);
    }

    /// A file that is not a recording has to say so rather than quietly
    /// blanking the window.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn loading_something_that_is_not_a_recording_reports_it() {
        let mut path = std::env::temp_dir();
        path.push(format!("spectroscope-notbin-{}.bin", std::process::id()));
        std::fs::write(&path, b"this is not a sweep").expect("a temp file can be written");

        let ctx = egui::Context::default();
        let mut app = SpectroScopeApp::build(&ctx, Config::default(), None);
        app.load_recording(path.clone());
        assert!(app.error.is_some(), "a junk file loaded silently");
        assert!(!app.storage.has_data());

        let missing = path.with_file_name("spectroscope-does-not-exist.bin");
        app.error = None;
        app.load_recording(missing);
        assert!(app.error.is_some(), "a missing file loaded silently");

        let _ = std::fs::remove_file(&path);
    }

    /// The bug this pins down: an `rtl_tcp` sweep of 34 hops takes about 7.7 s
    /// at the default 1 s interval, and the bar used to be scaled by the
    /// interval -- so one second in it was already past full and stayed there,
    /// marching, for the remaining six and a half.
    #[test]
    fn the_progress_bar_follows_the_measured_sweep_not_the_interval() {
        let (fraction, remaining) = sweep_progress(3.85, 7.7, 1.0).expect("a real reading");
        assert!((fraction - 0.5).abs() < 1e-9, "{fraction}");
        assert!((remaining - 3.85).abs() < 1e-9, "{remaining}");

        // Early in that same sweep the old code was already animating.
        let (fraction, _) = sweep_progress(1.1, 7.7, 1.0).expect("a real reading");
        assert!(fraction < 0.2, "{fraction}");

        // A sweep is not late merely for running a second over.
        assert!(sweep_progress(8.6, 7.7, 1.0).is_some());
        // Far enough past it, the remaining time really is unknown.
        assert!(sweep_progress(20.0, 7.7, 1.0).is_none());
    }

    /// Nothing measured yet, so the interval is all there is; and with neither,
    /// the bar has to animate rather than divide by zero.
    #[test]
    fn progress_falls_back_to_the_interval_then_to_nothing() {
        let (fraction, _) = sweep_progress(0.5, 0.0, 1.0).expect("the interval is usable");
        assert!((fraction - 0.5).abs() < 1e-9, "{fraction}");

        assert!(sweep_progress(0.5, 0.0, 0.0).is_none());
        assert!(sweep_progress(0.5, 0.0, f64::NAN).is_none());
        assert!(sweep_progress(f64::NAN, 2.0, 1.0).is_none());

        // Free-running (interval 0) is fine once a sweep has been timed, which
        // the old `interval < 1.0` test could never reach.
        assert!(sweep_progress(0.1, 0.25, 0.0).is_some());
    }

    /// The negative sentinel has to read as "auto" wherever it is shown, or the
    /// drop-down would offer a nonsensical "-1.0 dB".
    #[test]
    fn gain_labels_spell_the_auto_sentinel_out() {
        assert_eq!(gain_label(-1.0), "auto");
        assert_eq!(gain_label(0.0), "0.0 dB");
        assert_eq!(gain_label(37.2), "37.2 dB");
        assert_eq!(gain_label(49.6), "49.6 dB");

        let labels: Vec<String> = crate::sources::RTL_GAINS
            .iter()
            .map(|g| gain_label(*g))
            .collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "two gains share a label");
    }

    /// Lay the side panel out at `width` and report the width it claims back.
    ///
    /// egui stores a panel's width from the rect its *contents* ended up
    /// needing, so a row that cannot shrink is a floor the panel springs back
    /// to. Two passes, because the first is what the second reads.
    fn panel_width_after_laying_out_at(width: f32) -> f32 {
        panel_width_with(Config::default(), width)
    }

    fn panel_width_with(cfg: Config, width: f32) -> f32 {
        probe_panel(cfg, width, |app, ctx| app.side_panel(ctx))
    }

    /// The longest text the panel can be made to show: the widest colour map
    /// name, the widest band plan label, the longer record format and a path
    /// that will not fit in any panel.
    fn worst_case_config() -> Config {
        let mut cfg = Config::default();
        cfg.levels.colormap = crate::colormap::NAMES
            .iter()
            .enumerate()
            .max_by_key(|(_, n)| n.len())
            .map(|(i, _)| i)
            .unwrap_or(0);
        if let Some(plan) = crate::bandplan::plans()
            .iter()
            .max_by_key(|p| p.label.len())
        {
            cfg.band_plan = plan.id.to_owned();
        }
        cfg.show_band_plan = true;
        cfg.record_format = crate::data::RecordFormat::ALL
            .into_iter()
            .max_by_key(|f| f.label().len())
            .unwrap_or(cfg.record_format);
        cfg.record_file = format!("C:/{}/sweeps.bin", "a-very-long-directory-name/".repeat(8));
        cfg
    }

    /// As above, but for one section shown on its own, so a collapsed one can
    /// still be measured.
    fn section_width_at(
        cfg: Config,
        width: f32,
        mut section: impl FnMut(&mut SpectroScopeApp, &mut egui::Ui),
    ) -> f32 {
        probe_panel(cfg, width, move |app, ctx| {
            egui::SidePanel::right("spectroscope.controls")
                .resizable(true)
                .width_range(MIN_PANEL_WIDTH..=520.0)
                .show(ctx, |ui| section(app, ui));
        })
    }

    /// The width a first run gets in a `window`-wide window: no stored panel
    /// state, so `default_width` is what decides.
    fn first_run_panel_width(window: f32) -> f32 {
        let ctx = egui::Context::default();
        let mut app = SpectroScopeApp::build(&ctx, Config::default(), None);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(window, 700.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.side_panel(ctx));
        egui::panel::PanelState::load(&ctx, egui::Id::new("spectroscope.controls"))
            .expect("the panel stores its state")
            .rect
            .width()
    }

    fn probe_panel(
        cfg: Config,
        width: f32,
        mut show: impl FnMut(&mut SpectroScopeApp, &egui::Context),
    ) -> f32 {
        let ctx = egui::Context::default();
        let id = egui::Id::new("spectroscope.controls");
        let mut app = SpectroScopeApp::build(&ctx, cfg, None);

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };

        for _ in 0..2 {
            // Pretend the user just dragged the edge to `width`.
            ctx.data_mut(|d| {
                d.insert_persisted(
                    id,
                    egui::panel::PanelState {
                        rect: egui::Rect::from_min_size(
                            egui::pos2(1000.0 - width, 0.0),
                            egui::vec2(width, 700.0),
                        ),
                    },
                )
            });
            let _ = ctx.run(input.clone(), |ctx| show(&mut app, ctx));
        }

        egui::panel::PanelState::load(&ctx, id)
            .expect("the panel stores its state")
            .rect
            .width()
    }

    #[test]
    fn the_side_panel_keeps_the_width_it_is_dragged_to() {
        // The bug: a row demanding more than the panel offered (a fixed-width
        // combo, or a text field asking for the full width *next to* a button)
        // was written back as the panel's width, so every drag inwards sprang
        // straight back out again.
        for width in [190.0f32, 220.0, 260.0, 400.0] {
            let got = panel_width_after_laying_out_at(width);
            assert!(
                (got - width).abs() <= 1.0,
                "asked for {width}, panel claimed {got}"
            );
        }
    }

    /// A first run gets a fifth of the window, not a fixed number of points.
    #[test]
    fn the_panel_defaults_to_a_fifth_of_the_window() {
        for window in [1280.0f32, 1920.0, 2560.0, 3840.0] {
            let got = first_run_panel_width(window);
            let want = window * PANEL_WIDTH_FRACTION;
            assert!(
                (got - want).abs() <= 1.0,
                "{window} wide window: wanted {want}, panel took {got}"
            );
        }
    }

    /// ...except where a fifth would be narrower than the contents, which is
    /// every window below about 950 points -- a real case on a laptop with the
    /// window not maximised.
    #[test]
    fn a_narrow_window_still_gets_a_usable_panel() {
        for window in [480.0f32, 700.0, 900.0] {
            let got = first_run_panel_width(window);
            assert!(
                got >= MIN_PANEL_WIDTH - 1.0 && got <= window,
                "{window} wide window: panel took {got}"
            );
        }
    }

    /// And a window narrower than the panel's own minimum must not panic.
    ///
    /// Not hypothetical: a web canvas is a few pixels wide for the first frame
    /// or two while the page sizes it, and the vertical split used to abort
    /// the whole application there. egui panics on an inverted `Rangef`, which
    /// is exactly what a naive `min..=window * 0.5` would produce here.
    #[test]
    fn an_absurdly_narrow_window_does_not_panic() {
        for window in [0.0f32, 1.0, 50.0, 189.0, MIN_PANEL_WIDTH] {
            let got = first_run_panel_width(window);
            assert!(got.is_finite() && got >= 0.0, "{window} -> {got}");
        }
    }

    /// The same, with every selectable label at its longest. A combo sizes
    /// itself to its selected text unless told to truncate, so "Magma" fitting
    /// says nothing about the widest colour map in the list.
    #[test]
    fn the_longest_labels_do_not_widen_the_panel() {
        for width in [190.0f32, 260.0, 400.0] {
            let got = panel_width_with(worst_case_config(), width);
            assert!(
                (got - width).abs() <= 1.0,
                "asked for {width}, panel claimed {got}"
            );
        }
    }

    /// The *Recording* section starts collapsed, so the panel test above never
    /// reaches it -- and its file row, a text field beside a browse button, is
    /// where the width used to run away a few points every frame.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_recording_section_fits_the_panel_too() {
        for cfg in [Config::default(), worst_case_config()] {
            for width in [190.0f32, 260.0, 400.0] {
                let got =
                    section_width_at(cfg.clone(), width, |app, ui| app.recording_controls(ui));
                assert!(
                    (got - width).abs() <= 1.0,
                    "asked for {width}, recording claimed {got}"
                );
            }
        }
    }

    /// A zoom with no new sweep must still re-decimate the curves.
    ///
    /// Decimation is done against the visible range, so moving the view
    /// invalidates the cached points exactly as new data does. Syncing only
    /// when a sweep had arrived left the trace clipped to the previous window
    /// until the next one landed -- a whole second at one sweep per second,
    /// which reads as the plot flickering while you zoom.
    ///
    /// Driven through the real `App::update`, so it is the decision to sync
    /// that is under test and not just the flag behind it.
    #[test]
    fn zooming_without_new_data_still_resyncs_the_curves() {
        use eframe::App as _;

        let ctx = egui::Context::default();
        let mut app = SpectroScopeApp::build(&ctx, Config::default(), None);
        let mut frame = eframe::Frame::_new_kittest();

        let bins = 512usize;
        let x: Vec<f64> = (0..bins).map(|i| 87.0e6 + i as f64 * 1.0e4).collect();
        let y: Vec<f32> = (0..bins).map(|i| -90.0 + (i % 7) as f32).collect();
        app.storage
            .update(Frame {
                timestamp: 1.0,
                x: std::sync::Arc::new(x.clone()),
                y,
            })
            .expect("the first sweep sets the bin count");

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let mut draw = |app: &mut SpectroScopeApp| {
            let _ = ctx.run(input.clone(), |ctx| app.update(ctx, &mut frame));
        };

        for _ in 0..3 {
            draw(&mut app);
        }
        let full = app.spectrum.main_span().expect("a curve was decimated");
        assert!(full.1 - full.0 > 4.0e6, "{full:?} should span the sweep");

        // Zoom to a fiftieth of the sweep, the way the waterfall does, and
        // deliver no data at all afterwards.
        app.spectrum.set_view_x(x[100], x[200]);
        for _ in 0..2 {
            draw(&mut app);
        }

        let zoomed = app.spectrum.main_span().expect("a curve was decimated");
        assert!(
            zoomed.0 >= x[99] && zoomed.1 <= x[201],
            "{zoomed:?} should follow the zoom without waiting for a sweep, \
             but it still spans {full:?}"
        );
    }

    #[test]
    fn sweep_time_needs_two_sweeps_to_mean_anything() {
        assert_eq!(average_sweep_time(5.0, 5.0, 0), 0.0);
        assert_eq!(average_sweep_time(5.0, 5.0, 1), 0.0);
    }

    #[test]
    fn sweep_time_is_the_run_average() {
        // 11 sweeps spanning 10 s is one per second.
        assert!((average_sweep_time(110.0, 100.0, 11) - 1.0).abs() < 1e-9);
        assert!((average_sweep_time(110.0, 100.0, 101) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn a_delivery_burst_cannot_distort_the_rate() {
        // Ten sweeps over 10 s, then six more delivered in the same instant by
        // a tab coming back from the background: the average barely moves,
        // where the last-gap reading would have said 3000 FPS.
        let steady = average_sweep_time(110.0, 100.0, 11);
        let after_burst = average_sweep_time(110.0, 100.0, 17);
        assert!((steady - 1.0).abs() < 1e-9);
        assert!(after_burst > 0.6, "{after_burst}");
    }

    #[test]
    fn sweep_time_rejects_a_nonsense_clock() {
        assert_eq!(average_sweep_time(100.0, 110.0, 5), 0.0);
        assert_eq!(average_sweep_time(f64::NAN, 100.0, 5), 0.0);
    }

    #[test]
    fn human_bytes_picks_a_unit() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 kB");
        assert_eq!(human_bytes(1_572_864), "1.5 MB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
        // Never runs past the last unit.
        assert!(human_bytes(u64::MAX).ends_with(" GB"));
    }

    #[test]
    fn split_height_never_panics_on_a_small_window() {
        // The clamp form this replaced aborted the whole app here.
        for total in [0.0f32, 1.0, 7.0, 50.0, 95.0, 103.0, 160.0, 800.0] {
            for fraction in [0.0f32, 0.1, 0.5, 0.9, 1.0] {
                let h = split_height(total, 7.0, fraction);
                assert!(
                    h.is_finite() && h >= 0.0,
                    "total {total}, f {fraction} -> {h}"
                );
                assert!(h <= total.max(0.0), "total {total} -> {h}");
            }
        }
    }

    #[test]
    fn split_height_honours_the_fraction_when_there_is_room() {
        let h = split_height(1000.0, 7.0, 0.25);
        assert!((h - 250.0).abs() < 1e-3, "{h}");
        assert!(split_height(1000.0, 7.0, 0.0) >= MIN_PANE_HEIGHT);
        assert!(split_height(1000.0, 7.0, 1.0) <= 1000.0 - 7.0 - MIN_PANE_HEIGHT);
    }

    #[test]
    fn merge_is_a_union() {
        let a = Updated {
            data: true,
            ..Default::default()
        };
        let b = Updated {
            average: true,
            recalculated: true,
            ..Default::default()
        };
        let m = merge(a, b);
        assert!(m.data && m.average && m.recalculated);
        assert!(!m.peak_hold_max);
        assert_eq!(
            merge(Updated::default(), Updated::default()),
            Updated::default()
        );
        assert_eq!(merge(Updated::ALL, Updated::default()), Updated::ALL);
    }

    #[test]
    fn window_title_has_the_version() {
        let t = window_title();
        assert!(t.starts_with(APP_NAME));
        assert!(t.contains(VERSION));
    }
}
