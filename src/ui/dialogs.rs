//! The modal windows: Settings, Smoothing, Persistence, Colors, Baseline,
//! the backend help viewer and About.
//!
//! Replaces `settings.py`, `smoothing.py`, `persistence.py`, `colors.py` and
//! `baseline.py`. Each Qt dialog edited `QSettings` directly and the main window
//! re-read it on accept; here each window edits a scratch copy of [`Config`] and
//! reports what the application has to act on through [`DialogOutcome`], because
//! a backend change needs the acquisition restarted while a colour change does
//! not.

use crate::config::{Colors, Config, DecayFn, Rgba8};
use crate::dsp::smooth::SmoothWindow;
use crate::sources::{self, SourceKind};
use crate::{APP_NAME, VERSION};

/// What the application must do after a dialog was accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DialogOutcome {
    /// The backend or its device parameters changed: re-seed limits and
    /// recreate the source.
    pub backend_changed: bool,
    /// Smoothing parameters changed: the derived series must be recomputed.
    pub smoothing_changed: bool,
    /// The baseline file changed: reload it.
    pub baseline_changed: bool,
    /// The persistence fan length changed: refill it from history.
    pub persistence_changed: bool,
    /// The waterfall history size changed: resize the ring and the texture.
    pub history_size_changed: bool,
}

impl DialogOutcome {
    pub fn any(&self) -> bool {
        self.backend_changed
            || self.smoothing_changed
            || self.baseline_changed
            || self.persistence_changed
            || self.history_size_changed
    }
}

/// Which window is open. At most one at a time, like the Qt modal dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Settings,
    Smoothing,
    Persistence,
    Colors,
    Baseline,
    About,
}

#[derive(Default)]
pub struct Dialogs {
    open: Option<Which>,
    /// Edits are applied to this copy and only committed on OK, so Cancel
    /// really cancels.
    draft: Config,
    /// Output of the backend help buttons, shown in a scrolling pane.
    help_text: Option<(String, String)>,
    /// Pending async file-picker result, polled from the GUI thread.
    picked_baseline: Option<String>,
}

impl Dialogs {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn open_settings(&mut self, cfg: &Config) {
        self.draft = cfg.clone();
        self.open = Some(Which::Settings);
    }

    pub fn open_smoothing(&mut self, cfg: &Config) {
        self.draft = cfg.clone();
        self.open = Some(Which::Smoothing);
    }

    pub fn open_persistence(&mut self, cfg: &Config) {
        self.draft = cfg.clone();
        self.open = Some(Which::Persistence);
    }

    pub fn open_colors(&mut self, cfg: &Config) {
        self.draft = cfg.clone();
        self.open = Some(Which::Colors);
    }

    pub fn open_baseline(&mut self, cfg: &Config) {
        self.draft = cfg.clone();
        self.open = Some(Which::Baseline);
    }

    pub fn open_about(&mut self) {
        self.open = Some(Which::About);
    }

    /// Draw whichever window is open and commit it into `cfg` if accepted.
    pub fn ui(&mut self, ctx: &egui::Context, cfg: &mut Config) -> DialogOutcome {
        let Some(which) = self.open else {
            return DialogOutcome::default();
        };

        let mut outcome = DialogOutcome::default();
        let mut keep_open = true;
        let mut accepted = false;

        let title = match which {
            Which::Settings => "Settings",
            Which::Smoothing => "Smoothing",
            Which::Persistence => "Persistence",
            Which::Colors => "Colors",
            Which::Baseline => "Baseline",
            Which::About => "About",
        };

        egui::Window::new(format!("{title} - {APP_NAME}"))
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(which == Which::Settings)
            .default_width(if which == Which::Settings {
                520.0
            } else {
                360.0
            })
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let body = match which {
                    Which::Settings => self.settings_body(ui),
                    Which::Smoothing => self.smoothing_body(ui),
                    Which::Persistence => self.persistence_body(ui),
                    Which::Colors => self.colors_body(ui),
                    Which::Baseline => self.baseline_body(ui),
                    Which::About => {
                        about_body(ui);
                        DialogOutcome::default()
                    }
                };

                ui.separator();
                ui.horizontal(|ui| {
                    if which == Which::About {
                        if ui.button("Close").clicked() {
                            self.open = None;
                        }
                        return;
                    }
                    if ui.button("OK").clicked() {
                        accepted = true;
                        outcome = body;
                    }
                    if ui.button("Cancel").clicked() {
                        self.open = None;
                    }
                });
            });

        if accepted {
            *cfg = self.draft.clone();
            cfg.clamp_to(&cfg.source().info().limits);
            self.open = None;
        }
        if !keep_open {
            self.open = None;
        }

        self.help_window(ctx);
        outcome
    }

    // --- Settings ---------------------------------------------------------

    fn settings_body(&mut self, ui: &mut egui::Ui) -> DialogOutcome {
        let before = self.draft.clone();
        let mut pending_help: Option<(String, String)> = None;

        egui::Grid::new("settings.grid")
            .num_columns(3)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Backend:");
                let current = self.draft.source();
                egui::ComboBox::from_id_salt("settings.backend")
                    .selected_text(current.info().label)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for source in sources::registry() {
                            let info = source.info();
                            let selected = info.id == self.draft.backend;
                            if ui.selectable_label(selected, info.label).clicked() && !selected {
                                // Changing the backend re-seeds every tunable
                                // from that backend's own defaults, which is
                                // what setup_power_thread() did in the Qt build.
                                self.draft.apply_backend_defaults(*source);
                            }
                        }
                    });
                ui.end_row();

                let info = self.draft.source().info();
                let kind = info.kind;

                if kind == SourceKind::Process {
                    ui.label("Executable:");
                    ui.text_edit_singleline(&mut self.draft.executable);
                    if ui.button("...").on_hover_text("Browse").clicked() {
                        if let Some(p) = pick_file("Select executable") {
                            self.draft.executable = p;
                        }
                    }
                    ui.end_row();
                }

                match kind {
                    SourceKind::Network => {
                        ui.label("Device:")
                            .on_hover_text("host:port of the rtl_tcp server");
                        ui.text_edit_singleline(&mut self.draft.device);
                        ui.label("");
                        ui.end_row();
                    }
                    SourceKind::Process => {
                        ui.label("Device:");
                        ui.text_edit_singleline(&mut self.draft.device);
                        if ui
                            .add_enabled(info.has_device_help, egui::Button::new(" ? "))
                            .on_hover_text("Query the backend for available devices")
                            .clicked()
                        {
                            let text = self
                                .draft
                                .source()
                                .device_help(&self.draft.executable, &self.draft.device)
                                .unwrap_or_else(|| "This backend has no device help.".to_owned());
                            pending_help = Some(("Device".to_owned(), text));
                        }
                        ui.end_row();
                    }
                    SourceKind::Synthetic => {}
                }

                let limits = info.limits;

                if !limits.sample_rate.is_fixed() {
                    ui.label("Sample rate:");
                    let mut mhz = self.draft.sample_rate / 1e6;
                    if ui
                        .add(
                            egui::DragValue::new(&mut mhz)
                                .suffix(" MHz")
                                .speed(0.01)
                                .range(
                                    (limits.sample_rate.min / 1e6)..=(limits.sample_rate.max / 1e6),
                                ),
                        )
                        .changed()
                    {
                        self.draft.sample_rate = mhz * 1e6;
                    }
                    ui.label("");
                    ui.end_row();
                }

                if !limits.bandwidth.is_fixed() {
                    ui.label("Bandwidth:");
                    let mut mhz = self.draft.bandwidth / 1e6;
                    if ui
                        .add(
                            egui::DragValue::new(&mut mhz)
                                .suffix(" MHz")
                                .speed(0.01)
                                .range((limits.bandwidth.min / 1e6)..=(limits.bandwidth.max / 1e6)),
                        )
                        .changed()
                    {
                        self.draft.bandwidth = mhz * 1e6;
                    }
                    ui.label("");
                    ui.end_row();
                }

                ui.label("LNB LO:").on_hover_text(
                    "Negative frequency for upconverters, positive for downconverters.",
                );
                let mut mhz = self.draft.lnb_lo / 1e6;
                if ui
                    .add(
                        egui::DragValue::new(&mut mhz)
                            .suffix(" MHz")
                            .speed(1.0)
                            .range(-100_000.0..=100_000.0),
                    )
                    .changed()
                {
                    self.draft.lnb_lo = mhz * 1e6;
                }
                ui.label("");
                ui.end_row();

                ui.label("Waterfall history size:");
                ui.add(
                    egui::DragValue::new(&mut self.draft.waterfall_history_size)
                        .speed(1.0)
                        .range(1..=20_000),
                );
                ui.label("");
                ui.end_row();

                if kind == SourceKind::Process {
                    ui.label("Additional parameters:");
                    ui.text_edit_singleline(&mut self.draft.params);
                    if ui
                        .button(" ? ")
                        .on_hover_text("Show the backend's own help")
                        .clicked()
                    {
                        let text = self.draft.source().params_help(&self.draft.executable);
                        pending_help = Some(("Additional parameters".to_owned(), text));
                    }
                    ui.end_row();
                }
            });

        if let Some(h) = pending_help {
            self.help_text = Some(h);
        }

        let d = &self.draft;
        DialogOutcome {
            backend_changed: d.backend != before.backend
                || d.executable != before.executable
                || d.params != before.params
                || d.device != before.device
                || d.sample_rate != before.sample_rate
                || d.bandwidth != before.bandwidth
                || d.lnb_lo != before.lnb_lo,
            history_size_changed: d.waterfall_history_size != before.waterfall_history_size,
            ..Default::default()
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let Some((title, text)) = self.help_text.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new(format!("{title} help - {APP_NAME}"))
            .open(&mut open)
            .default_size([760.0, 520.0])
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    // Backend help is argparse output: monospace, unwrapped.
                    ui.add(
                        egui::Label::new(egui::RichText::new(&text).monospace())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
            });
        if !open {
            self.help_text = None;
        }
    }

    // --- Smoothing --------------------------------------------------------

    fn smoothing_body(&mut self, ui: &mut egui::Ui) -> DialogOutcome {
        let before = (self.draft.smooth_window, self.draft.smooth_length);

        egui::Grid::new("smoothing.grid")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Window function:");
                egui::ComboBox::from_id_salt("smoothing.window")
                    .selected_text(self.draft.smooth_window.label())
                    .show_ui(ui, |ui| {
                        for w in SmoothWindow::ALL {
                            ui.selectable_value(&mut self.draft.smooth_window, w, w.label());
                        }
                    });
                ui.end_row();

                ui.label("Window length:");
                ui.add(
                    egui::DragValue::new(&mut self.draft.smooth_length)
                        .speed(1.0)
                        .range(1..=1001),
                );
                ui.end_row();
            });

        ui.label(
            egui::RichText::new(
                "A window shorter than 3 bins, or longer than the sweep, leaves the \
                 trace untouched.",
            )
            .small()
            .weak(),
        );

        DialogOutcome {
            smoothing_changed: (self.draft.smooth_window, self.draft.smooth_length) != before,
            ..Default::default()
        }
    }

    // --- Persistence ------------------------------------------------------

    fn persistence_body(&mut self, ui: &mut egui::Ui) -> DialogOutcome {
        let before = self.draft.persistence_length;

        egui::Grid::new("persistence.grid")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Decay function:");
                egui::ComboBox::from_id_salt("persistence.decay")
                    .selected_text(self.draft.persistence_decay.label())
                    .show_ui(ui, |ui| {
                        for d in DecayFn::ALL {
                            ui.selectable_value(&mut self.draft.persistence_decay, d, d.label());
                        }
                    });
                ui.end_row();

                ui.label("Persistence length:");
                ui.add(
                    egui::DragValue::new(&mut self.draft.persistence_length)
                        .speed(1.0)
                        .range(1..=100),
                );
                ui.end_row();
            });

        DialogOutcome {
            // Only the length needs the fan rebuilt; a different decay curve
            // just re-colours the curves already there.
            persistence_changed: self.draft.persistence_length != before,
            ..Default::default()
        }
    }

    // --- Colors -----------------------------------------------------------

    fn colors_body(&mut self, ui: &mut egui::Ui) -> DialogOutcome {
        egui::Grid::new("colors.grid")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                let rows: [ColorRow; 6] = [
                    ("Main curve color:", |c| &mut c.main),
                    ("Max. peak hold color:", |c| &mut c.peak_hold_max),
                    ("Min. peak hold color:", |c| &mut c.peak_hold_min),
                    ("Average color:", |c| &mut c.average),
                    ("Persistence color:", |c| &mut c.persistence),
                    ("Baseline color:", |c| &mut c.baseline),
                ];
                for (label, field) in rows {
                    ui.label(label);
                    color_button(ui, field(&mut self.draft.colors));
                    ui.end_row();
                }

                ui.label("");
                if ui.button("Reset to defaults").clicked() {
                    self.draft.colors = Colors::default();
                }
                ui.end_row();
            });

        // Colours are picked up from the config on the next repaint, so there
        // is nothing for the application to do.
        DialogOutcome::default()
    }

    // --- Baseline ---------------------------------------------------------

    fn baseline_body(&mut self, ui: &mut egui::Ui) -> DialogOutcome {
        let before = self.draft.baseline_file.clone();

        if let Some(p) = self.picked_baseline.take() {
            self.draft.baseline_file = p;
        }

        ui.horizontal(|ui| {
            ui.label("Baseline file:");
            ui.text_edit_singleline(&mut self.draft.baseline_file);
            if ui.button("...").clicked() {
                if let Some(p) = pick_file("Select baseline file") {
                    self.draft.baseline_file = p;
                }
            }
        });
        if !self.draft.baseline_file.is_empty() && ui.button("Clear").clicked() {
            self.draft.baseline_file.clear();
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "A soapy_power binary file (-F soapy_power_bin). Several sweeps in \
                 one file are averaged. The baseline must have the same number of \
                 bins as the live sweep.",
            )
            .small()
            .weak(),
        );

        DialogOutcome {
            baseline_changed: self.draft.baseline_file != before,
            ..Default::default()
        }
    }
}

fn about_body(ui: &mut egui::Ui) {
    ui.heading(format!("{APP_NAME} {VERSION}"));
    ui.add_space(4.0);
    ui.label("Spectrum analyzer for multiple SDR platforms.");
    ui.add_space(8.0);

    egui::Grid::new("about.grid").num_columns(2).show(ui, |ui| {
        ui.label("Renderer:");
        ui.label("wgpu");
        ui.end_row();
        ui.label("FFT:");
        ui.label(crate::dsp::fft_backend_name());
        ui.end_row();
        ui.label("Backends:");
        ui.label(
            sources::registry()
                .iter()
                .map(|s| s.info().id)
                .collect::<Vec<_>>()
                .join(", "),
        );
        ui.end_row();
    });

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("A Rust/egui rewrite of QSpectrumAnalyzer by Michal Krenek (Mikos).")
            .small()
            .weak(),
    );
}

/// A label and the colour field it edits.
type ColorRow = (&'static str, fn(&mut Colors) -> &mut Rgba8);

/// An RGBA colour button that round-trips through [`Rgba8`] without drifting.
///
/// egui edits colours as unmultiplied `f32`, so going through `Color32` on
/// every frame would quantise repeatedly; this only writes back on change.
fn color_button(ui: &mut egui::Ui, color: &mut Rgba8) {
    let [r, g, b, a] = color.0;
    let mut rgba = [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ];
    if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
        color.0 = [
            (rgba[0] * 255.0).round().clamp(0.0, 255.0) as u8,
            (rgba[1] * 255.0).round().clamp(0.0, 255.0) as u8,
            (rgba[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            (rgba[3] * 255.0).round().clamp(0.0, 255.0) as u8,
        ];
    }
}

/// Native file chooser. Returns `None` on the web, where picking a path is
/// meaningless -- the user types or drops a file instead.
fn pick_file(title: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .pick_file()
            .map(|p| p.to_string_lossy().into_owned())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = title;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_discards_edits() {
        let mut cfg = Config::default();
        let original = cfg.clone();
        let mut d = Dialogs::default();
        d.open_smoothing(&cfg);
        d.draft.smooth_length = 99;
        // Closing without accepting must not touch the real config.
        d.open = None;
        assert_eq!(cfg, original);
        assert!(!d.is_open());
        // Guard against the draft being mistaken for the live config.
        assert_ne!(d.draft.smooth_length, cfg.smooth_length);
        cfg.smooth_length = 99;
        assert_eq!(d.draft.smooth_length, cfg.smooth_length);
    }

    #[test]
    fn opening_a_dialog_snapshots_the_config() {
        let mut cfg = Config::default();
        cfg.smooth_length = 21;
        let mut d = Dialogs::default();
        d.open_smoothing(&cfg);
        assert_eq!(d.draft.smooth_length, 21);
        assert!(d.is_open());
    }

    #[test]
    fn backend_switch_reseeds_the_draft() {
        let mut d = Dialogs::default();
        d.open_settings(&Config::default());

        let demo = sources::find("demo").expect("demo backend is always compiled in");
        d.draft.apply_backend_defaults(demo);

        assert_eq!(d.draft.backend, "demo");
        let limits = demo.info().limits;
        assert_eq!(d.draft.bin_size, limits.bin_size.default);
        assert_eq!(d.draft.gain, limits.gain.default);
        assert!(d.draft.device.is_empty());
    }

    #[test]
    fn outcome_any_reports_each_flag() {
        assert!(!DialogOutcome::default().any());
        for o in [
            DialogOutcome {
                backend_changed: true,
                ..Default::default()
            },
            DialogOutcome {
                smoothing_changed: true,
                ..Default::default()
            },
            DialogOutcome {
                baseline_changed: true,
                ..Default::default()
            },
            DialogOutcome {
                persistence_changed: true,
                ..Default::default()
            },
            DialogOutcome {
                history_size_changed: true,
                ..Default::default()
            },
        ] {
            assert!(o.any(), "{o:?}");
        }
    }

    #[test]
    fn colour_round_trip_is_stable() {
        // Repeated f32 conversions must not drift the stored bytes.
        for c in [
            Rgba8::new(255, 255, 0, 255),
            Rgba8::new(0, 0, 0, 0),
            Rgba8::new(1, 127, 128, 254),
        ] {
            let [r, g, b, a] = c.0;
            let rgba = [
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                a as f32 / 255.0,
            ];
            let back = [
                (rgba[0] * 255.0).round() as u8,
                (rgba[1] * 255.0).round() as u8,
                (rgba[2] * 255.0).round() as u8,
                (rgba[3] * 255.0).round() as u8,
            ];
            assert_eq!(back, c.0);
        }
    }

    #[test]
    fn only_one_window_is_open_at_a_time() {
        let cfg = Config::default();
        let mut d = Dialogs::default();
        d.open_settings(&cfg);
        d.open_colors(&cfg);
        assert_eq!(d.open, Some(Which::Colors));
    }
}
