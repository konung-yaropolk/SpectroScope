//! Band plan for Slovakia.
//!
//! Transcribed from the SDR++ band plan for Slovakia, fetched 2026-10-11.
//! Only deviation: CB and PMR446 are typed `other` upstream and are
//! `Unlicensed` here, per the BandKind docs. The plan is thin -- it has no
//! 630m or 60m entry -- but no Slovak source for those could be reached, so
//! nothing was added.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "slovakia",
    label: "Slovakia",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/slovakia.json",
    bands: &BANDS,
};

static BANDS: [Band; 18] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "LW",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 1_810_000.0,
        stop_hz: 2_000_000.0,
        name: "160m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_500_000.0,
        stop_hz: 3_800_000.0,
        name: "80m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 7_000_000.0,
        stop_hz: 7_200_000.0,
        name: "40m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_100_000.0,
        stop_hz: 10_150_000.0,
        name: "30m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 14_000_000.0,
        stop_hz: 14_350_000.0,
        name: "20m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 18_068_000.0,
        stop_hz: 18_168_000.0,
        name: "17m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 21_000_000.0,
        stop_hz: 21_450_000.0,
        name: "15m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 24_890_000.0,
        stop_hz: 24_990_000.0,
        name: "12m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 26_965_000.0,
        stop_hz: 27_405_000.0,
        name: "CB",
        kind: BandKind::Unlicensed,
    }, // licence-exempt, not `other` as typed upstream
    Band {
        start_hz: 28_000_000.0,
        stop_hz: 29_700_000.0,
        name: "10m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 50_000_000.0,
        stop_hz: 52_000_000.0,
        name: "6m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 87_500_000.0,
        stop_hz: 108_000_000.0,
        name: "FM",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 146_000_000.0,
        name: "2m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 440_000_000.0,
        name: "70cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 446_006_250.0,
        stop_hz: 446_193_750.0,
        name: "PMR446",
        kind: BandKind::Unlicensed,
    }, // licence-exempt, not `other` as typed upstream
    Band {
        start_hz: 1_240_000_000.0,
        stop_hz: 1_300_000_000.0,
        name: "23cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_300_000_000.0,
        stop_hz: 2_450_000_000.0,
        name: "13cm",
        kind: BandKind::Amateur,
    },
];
