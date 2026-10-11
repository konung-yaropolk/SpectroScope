//! Band plans: who is allocated what, so a sweep can be read against the
//! spectrum's official carve-up rather than guessed at.
//!
//! The plans are compiled in rather than loaded from disk. They are reference
//! data measured in kilobytes, they must be identical on every platform
//! including the web build that has no filesystem, and a band plan that fails
//! to load is worse than no band plan at all.
//!
//! # Adding a plan
//!
//! Write `src/bandplan/<region>.rs` exporting `pub static PLAN: BandPlan`, add
//! `pub mod <region>;` below, and list it in [`plans`]. The GUI picks it up
//! from there; nothing else needs touching. Bands must be sorted by
//! `start_hz` -- `every_plan_is_well_formed` enforces it.

pub mod australia;
pub mod austria;
pub mod belgium;
pub mod brazil;
pub mod canada;
pub mod china;
pub mod france;
pub mod general;
pub mod germany;
pub mod ireland;
pub mod italy;
pub mod korea;
pub mod netherlands;
pub mod qo100;
pub mod russia;
pub mod slovakia;
pub mod turkey;
pub mod uk;
pub mod usa;

/// What a band is for. Drives nothing but the colour a viewer might choose;
/// the name is what gets printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BandKind {
    Amateur,
    Broadcast,
    Aviation,
    Marine,
    Military,
    Satellite,
    /// Cellular and other licensed mobile services.
    Mobile,
    /// ISM, PMR, CB and similar licence-exempt allocations.
    Unlicensed,
    /// Point-to-point links, telemetry, radiolocation, time signals.
    Utility,
    Other,
}

impl BandKind {
    /// The `type` strings used by SDR++'s band plan files, so a transcription
    /// maps straight across.
    pub fn from_type(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "amateur" | "ham" => Self::Amateur,
            "broadcast" => Self::Broadcast,
            "aviation" | "air" => Self::Aviation,
            "marine" => Self::Marine,
            "military" => Self::Military,
            "satellite" => Self::Satellite,
            "mobile" | "cellular" => Self::Mobile,
            "ism" | "pmr" | "cb" | "unlicensed" => Self::Unlicensed,
            "utility" | "radiolocation" | "time" => Self::Utility,
            _ => Self::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Amateur => "amateur",
            Self::Broadcast => "broadcast",
            Self::Aviation => "aviation",
            Self::Marine => "marine",
            Self::Military => "military",
            Self::Satellite => "satellite",
            Self::Mobile => "mobile",
            Self::Unlicensed => "unlicensed",
            Self::Utility => "utility",
            Self::Other => "other",
        }
    }
}

/// One allocation.
#[derive(Clone, Copy, Debug)]
pub struct Band {
    /// Inclusive lower edge, Hz.
    pub start_hz: f64,
    /// Exclusive upper edge, Hz.
    pub stop_hz: f64,
    pub name: &'static str,
    pub kind: BandKind,
}

impl Band {
    pub fn width_hz(&self) -> f64 {
        self.stop_hz - self.start_hz
    }

    pub fn centre_hz(&self) -> f64 {
        (self.start_hz + self.stop_hz) * 0.5
    }

    /// True when this band shares any frequency with `lo..hi`.
    pub fn overlaps(&self, lo: f64, hi: f64) -> bool {
        self.start_hz < hi && self.stop_hz > lo
    }
}

/// A named set of allocations, usually one country's.
#[derive(Clone, Copy, Debug)]
pub struct BandPlan {
    /// Stable identifier used in the config file. Never translate this.
    pub id: &'static str,
    /// What the region combo box shows.
    pub label: &'static str,
    /// Where the data came from, so it can be rechecked.
    pub source: &'static str,
    /// Sorted by `start_hz`.
    pub bands: &'static [Band],
}

impl BandPlan {
    /// Bands sharing any frequency with `lo..hi`, in frequency order.
    pub fn overlapping(&self, lo: f64, hi: f64) -> impl Iterator<Item = &Band> {
        self.bands.iter().filter(move |b| b.overlaps(lo, hi))
    }
}

/// The empty plan, for "no band plan" in the GUI.
pub static NONE: BandPlan = BandPlan {
    id: "none",
    label: "None",
    source: "",
    bands: &[],
};

/// Every plan compiled in, in display order.
pub fn plans() -> &'static [&'static BandPlan] {
    ALL
}

static ALL: &[&BandPlan] = &[
    &NONE,
    &general::PLAN,
    &usa::PLAN,
    &canada::PLAN,
    &uk::PLAN,
    &ireland::PLAN,
    &france::PLAN,
    &germany::PLAN,
    &netherlands::PLAN,
    &belgium::PLAN,
    &austria::PLAN,
    &italy::PLAN,
    &slovakia::PLAN,
    &turkey::PLAN,
    &russia::PLAN,
    &china::PLAN,
    &korea::PLAN,
    &australia::PLAN,
    &brazil::PLAN,
    &qo100::PLAN,
];

/// Look a plan up by its [`BandPlan::id`], falling back to [`NONE`].
pub fn find(id: &str) -> &'static BandPlan {
    plans()
        .iter()
        .copied()
        .find(|p| p.id == id)
        .unwrap_or(&NONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plan_is_well_formed() {
        for plan in plans() {
            assert!(!plan.id.is_empty(), "a plan has no id");
            assert!(!plan.label.is_empty(), "{} has no label", plan.id);

            let mut previous = f64::NEG_INFINITY;
            for b in plan.bands {
                assert!(
                    b.start_hz.is_finite() && b.stop_hz.is_finite(),
                    "{}: {} has a non-finite edge",
                    plan.id,
                    b.name
                );
                assert!(
                    b.stop_hz > b.start_hz,
                    "{}: {} is empty or inverted ({} -> {})",
                    plan.id,
                    b.name,
                    b.start_hz,
                    b.stop_hz
                );
                assert!(
                    b.start_hz >= previous,
                    "{}: {} breaks the frequency ordering",
                    plan.id,
                    b.name
                );
                assert!(!b.name.is_empty(), "{}: a band has no name", plan.id);
                // Nothing in these plans belongs below DC or above 300 GHz.
                assert!(
                    b.start_hz >= 0.0 && b.stop_hz <= 300e9,
                    "{}: {} is out of plausible range",
                    plan.id,
                    b.name
                );
                previous = b.start_hz;
            }
        }
    }

    #[test]
    fn plan_ids_are_unique_and_findable() {
        let mut ids: Vec<&str> = plans().iter().map(|p| p.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate plan id");

        for plan in plans() {
            assert_eq!(find(plan.id).id, plan.id);
        }
        assert_eq!(find("no-such-plan").id, "none");
    }

    #[test]
    fn every_real_plan_cites_a_source() {
        for plan in plans().iter().filter(|p| !p.bands.is_empty()) {
            assert!(
                plan.source.starts_with("http"),
                "{} does not cite where its data came from",
                plan.id
            );
        }
    }

    #[test]
    fn overlap_is_half_open() {
        let b = Band {
            start_hz: 100.0,
            stop_hz: 200.0,
            name: "x",
            kind: BandKind::Other,
        };
        assert!(b.overlaps(150.0, 250.0));
        assert!(b.overlaps(50.0, 150.0));
        assert!(b.overlaps(0.0, 1000.0));
        // Touching edges do not count as overlapping.
        assert!(!b.overlaps(200.0, 300.0));
        assert!(!b.overlaps(0.0, 100.0));
        assert_eq!(b.width_hz(), 100.0);
        assert_eq!(b.centre_hz(), 150.0);
    }

    #[test]
    fn the_general_plan_covers_the_obvious_landmarks() {
        // A spot check against allocations that are the same everywhere, so a
        // badly mangled transcription cannot pass unnoticed.
        let plan = &general::PLAN;
        let has = |lo: f64, hi: f64| plan.overlapping(lo, hi).next().is_some();
        assert!(has(88e6, 108e6), "FM broadcast missing");
        assert!(has(144e6, 146e6), "2m amateur missing");
        assert!(has(120e6, 130e6), "airband missing");
        assert!(plan.bands.len() > 20, "the general plan looks truncated");
    }

    #[test]
    fn kinds_map_from_the_source_type_strings() {
        assert_eq!(BandKind::from_type("amateur"), BandKind::Amateur);
        assert_eq!(BandKind::from_type("Broadcast"), BandKind::Broadcast);
        assert_eq!(BandKind::from_type("  aviation "), BandKind::Aviation);
        assert_eq!(BandKind::from_type("nonsense"), BandKind::Other);
    }
}
