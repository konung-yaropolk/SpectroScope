//! Band plan for Belgium.
//!
//! Transcribed from the SDR++ band plan for Belgium, fetched 2026-10-11.
//! Only deviation: "11m - Citizen Band" and PMR446 are typed `amateur`
//! upstream; they are licence-exempt services, so they are `Unlicensed` here.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "belgium",
    label: "Belgium",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/belgium.json",
    bands: &BANDS,
};

static BANDS: [Band; 45] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "2200m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 472_000.0,
        stop_hz: 479_000.0,
        name: "630m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 501_000.0,
        stop_hz: 504_000.0,
        name: "600m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 526_500.0,
        stop_hz: 1_606_500.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 1_810_000.0,
        stop_hz: 2_000_000.0,
        name: "160m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_500_000.0,
        stop_hz: 3_800_000.0,
        name: "80m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_351_500.0,
        stop_hz: 5_366_500.0,
        name: "60m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_950_000.0,
        stop_hz: 6_200_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 7_000_000.0,
        stop_hz: 7_200_000.0,
        name: "40m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 7_200_000.0,
        stop_hz: 7_300_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 9_500_000.0,
        stop_hz: 9_900_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 10_100_000.0,
        stop_hz: 10_150_000.0,
        name: "30m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 11_650_000.0,
        stop_hz: 12_050_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 13_600_000.0,
        stop_hz: 13_800_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 14_000_000.0,
        stop_hz: 14_350_000.0,
        name: "20m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 15_100_000.0,
        stop_hz: 15_600_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 17_550_000.0,
        stop_hz: 17_900_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 18_068_000.0,
        stop_hz: 18_168_000.0,
        name: "17m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 21_000_000.0,
        stop_hz: 21_450_000.0,
        name: "15m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 21_450_000.0,
        stop_hz: 21_850_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 24_890_000.0,
        stop_hz: 24_990_000.0,
        name: "12m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 25_670_000.0,
        stop_hz: 26_100_000.0,
        name: "AM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 26_960_000.0,
        stop_hz: 27_410_000.0,
        name: "11m - Citizen Band",
        kind: BandKind::Unlicensed,
    }, // licence-exempt, not `amateur` as typed upstream
    Band {
        start_hz: 28_000_000.0,
        stop_hz: 29_700_000.0,
        name: "10m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 40_660_000.0,
        stop_hz: 40_690_000.0,
        name: "8m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 50_000_000.0,
        stop_hz: 52_000_000.0,
        name: "6m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 69_945_000.0,
        stop_hz: 69_955_000.0,
        name: "4m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 70_190_000.0,
        stop_hz: 70_412_500.0,
        name: "4m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 87_500_000.0,
        stop_hz: 108_000_000.0,
        name: "FM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 137_000_000.0,
        stop_hz: 138_000_000.0,
        name: "Space Exploration / Meteorology Sat. / S-PCS",
        kind: BandKind::Satellite,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 146_000_000.0,
        name: "2m - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 174_000_000.0,
        stop_hz: 223_000_000.0,
        name: "T-DAB Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 440_000_000.0,
        name: "70cm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 446_000_000.0,
        stop_hz: 446_200_000.0,
        name: "PMR446",
        kind: BandKind::Unlicensed,
    }, // licence-exempt, not `amateur` as typed upstream
    Band {
        start_hz: 470_000_000.0,
        stop_hz: 790_000_000.0,
        name: "DVB-T - Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 1_240_000_000.0,
        stop_hz: 1_300_000_000.0,
        name: "23cm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_300_000_000.0,
        stop_hz: 2_450_000_000.0,
        name: "13cm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_650_000_000.0,
        stop_hz: 5_850_000_000.0,
        name: "6cm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_000_000_000.0,
        stop_hz: 10_500_000_000.0,
        name: "3cm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 24_000_000_000.0,
        stop_hz: 24_250_000_000.0,
        name: "1.25cm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 47_000_000_000.0,
        stop_hz: 47_200_000_000.0,
        name: "6mm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 75_500_000_000.0,
        stop_hz: 81_000_000_000.0,
        name: "4mm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 122_250_000_000.0,
        stop_hz: 123_000_000_000.0,
        name: "2.5mm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 142_000_000_000.0,
        stop_hz: 149_000_000_000.0,
        name: "2mm - Amateur",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 241_000_000_000.0,
        stop_hz: 250_000_000_000.0,
        name: "1mm - Amateur",
        kind: BandKind::Amateur,
    },
];
