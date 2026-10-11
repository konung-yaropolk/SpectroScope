//! Band plan for QO-100 (Es'hail 2).
//!
//! Transcribed from https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/qo-100.json, fetched 2026-10-11.
//! Verbatim. This is the Es'hail-2 narrowband transponder, not a country:
//! upstream lists only the 10489.50-10490.00 MHz downlink segments, so that
//! is all that is here -- the 2400 MHz uplink is absent from the source and
//! was not invented to fill the gap.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "qo100",
    label: "QO-100 (Es'hail 2)",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/qo-100.json",
    bands: &BANDS,
};

static BANDS: [Band; 10] = [
    Band {
        start_hz: 10_489_500_000.0,
        stop_hz: 10_489_505_000.0,
        name: "Beacon",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 10_489_505_000.0,
        stop_hz: 10_489_540_000.0,
        name: "CW",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_540_000.0,
        stop_hz: 10_489_580_000.0,
        name: "NB Digi",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_580_000.0,
        stop_hz: 10_489_650_000.0,
        name: "Digi",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_650_000.0,
        stop_hz: 10_489_745_000.0,
        name: "SSB",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_745_000.0,
        stop_hz: 10_489_755_000.0,
        name: "Beacon",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 10_489_755_000.0,
        stop_hz: 10_489_850_000.0,
        name: "SSB",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_850_000.0,
        stop_hz: 10_489_870_000.0,
        name: "Emergency",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_870_000.0,
        stop_hz: 10_489_990_000.0,
        name: "Mixed/Contest",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_489_990_000.0,
        stop_hz: 10_490_000_000.0,
        name: "Beacon",
        kind: BandKind::Broadcast,
    },
];
