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
        cfg.clamp_to(&cfg.source().info().limits);

        let mut waterfall = Waterfall::new(cc.wgpu_render_state.clone());
        waterfall.set_colormap(cfg.levels.lut(), cfg.levels.reverse);

        apply_theme(&cc.egui_ctx, cfg.dark_mode);

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

    /// Write the stored history out as an image.
    #[cfg(not(target_arch = "wasm32"))]
    fn save_waterfall(&mut self, path: std::path::PathBuf) {
        {
            // The axes only mean anything alongside the pixels, so they
            // travel with them into the TIFF metadata.
            let axes = match self.storage.x() {
                Some(x) if x.len() > 1 => crate::data::image_export::Axes {
                    start_hz: x[0],
                    bin_hz: (x[x.len() - 1] - x[0]) / (x.len() - 1) as f64,
                    sweep_s: self.sweep_time,
                },
                _ => crate::data::image_export::Axes::default(),
            };

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
    /// Mirrors `update_progress()`: with no meaningful interval, or once the
    /// sweep is overdue, the bar animates instead of showing a fraction,
    /// because the remaining time is then unknown.
    fn progress_bar(&self) -> egui::ProgressBar {
        let elapsed = now_monotonic() - self.last_frame_at;
        let interval = self.cfg.interval;

        if interval < 1.0 || elapsed > interval + 1.0 {
            egui::ProgressBar::new(1.0)
                .desired_width(250.0)
                .animate(true)
                .text("")
        } else {
            let fraction = (elapsed / interval).clamp(0.0, 1.0) as f32;
            egui::ProgressBar::new(fraction)
                .desired_width(250.0)
                .text(format!("{:.1} s", (interval - elapsed).max(0.0)))
        }
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("spectroscope.controls")
            .resizable(true)
            .default_width(260.0)
            .width_range(220.0..=460.0)
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
                // One button that reports the state it will leave you in,
                // rather than two of which one is always dead.
                let label = if self.running { "Stop" } else { "Start" };
                if ui
                    .add(
                        egui::Button::new(label)
                            .min_size(egui::vec2(ui.available_width(), 28.0)),
                    )
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
                        egui::Button::new("Single shot")
                            .min_size(egui::vec2(ui.available_width(), 24.0)),
                    )
                    .clicked()
                {
                    self.start(ctx, true);
                }
                if ui
                    .add(
                        egui::Button::new("Reset view")
                            .min_size(egui::vec2(ui.available_width(), 24.0)),
                    )
                    .on_hover_text(
                        "Fit the spectrum to the sweep and return the waterfall to the newest                          sweep at its default scale",
                    )
                    .clicked()
                {
                    self.reset_view();
                }
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
                // A browser has no path-addressable filesystem, so there is
                // nothing to point a recorder at; saying so beats offering
                // controls whose file picker can only ever return nothing.
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label(
                        egui::RichText::new(
                            "Recording and image export need a filesystem, so they are                              only available in the desktop build.",
                        )
                        .small()
                        .weak(),
                    );
                    return;
                }

                #[cfg(not(target_arch = "wasm32"))]
                self.recording_controls(ui);
            });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recording_controls(&mut self, ui: &mut egui::Ui) {
        let recording = self.recorder.is_some();

        ui.horizontal(|ui| {
            ui.label("File:");
            ui.add_enabled(
                !recording,
                egui::TextEdit::singleline(&mut self.cfg.record_file)
                    .desired_width(ui.available_width() - 32.0),
            );
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
        });

        ui.horizontal(|ui| {
            ui.label("Format:");
            egui::ComboBox::from_id_salt("record.format")
                .selected_text(self.cfg.record_format.label())
                .width(190.0)
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
            .add(egui::Button::new(label).min_size(egui::vec2(ui.available_width(), 24.0)))
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
                egui::Button::new("Save waterfall...")
                    .min_size(egui::vec2(ui.available_width(), 24.0)),
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
        if updated.any() {
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

/// Native save dialog.
#[cfg(not(target_arch = "wasm32"))]
fn save_file_dialog(
    title: &str,
    current: &str,
    filters: &[(&str, &[&str])],
    default_ext: &str,
) -> Option<String> {
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
