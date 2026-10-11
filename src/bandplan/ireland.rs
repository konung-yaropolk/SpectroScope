//! Band plan for Ireland.
//!
//! Transcribed from https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/ireland.json, fetched 2026-10-11.
//! Corrected the Long Wave upper edge to 283.5 kHz and typed CB as unlicensed; see the comments on those two bands.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "ireland",
    label: "Ireland",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/ireland.json",
    bands: &BANDS,
};

static BANDS: [Band; 37] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "2200m Ham Band",
        kind: BandKind::Amateur,
    },
    // upper edge is 283.5 kHz per ITU RR Region 1; upstream's 282500 is a typo -- SDR++ germany.json and thargas International/BandPlan.xml both end LW here
    Band {
        start_hz: 148_500.0,
        stop_hz: 283_500.0,
        name: "Long wave",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 531_000.0,
        stop_hz: 1_602_000.0,
        name: "AM broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 1_810_000.0,
        stop_hz: 2_000_000.0,
        name: "160m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_300_000.0,
        stop_hz: 2_495_000.0,
        name: "120m SW broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 3_200_000.0,
        stop_hz: 3_400_000.0,
        name: "90m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 3_500_000.0,
        stop_hz: 3_800_000.0,
        name: "80m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_900_000.0,
        stop_hz: 4_000_000.0,
        name: "75m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 4_750_000.0,
        stop_hz: 5_060_000.0,
        name: "60m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 5_351_500.0,
        stop_hz: 5_366_500.0,
        name: "60m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_900_000.0,
        stop_hz: 6_200_000.0,
        name: "49m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 7_000_000.0,
        stop_hz: 7_200_000.0,
        name: "40m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 7_200_000.0,
        stop_hz: 7_450_000.0,
        name: "40m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 9_400_000.0,
        stop_hz: 9_900_000.0,
        name: "31m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 10_100_000.0,
        stop_hz: 10_150_000.0,
        name: "30m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 11_600_000.0,
        stop_hz: 12_100_000.0,
        name: "25m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 13_570_000.0,
        stop_hz: 13_870_000.0,
        name: "22m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 14_000_000.0,
        stop_hz: 14_350_000.0,
        name: "20m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 15_100_000.0,
        stop_hz: 15_800_000.0,
        name: "19m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 17_480_000.0,
        stop_hz: 17_900_000.0,
        name: "16m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 18_068_000.0,
        stop_hz: 18_168_000.0,
        name: "17m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 18_900_000.0,
        stop_hz: 19_020_000.0,
        name: "15m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 21_000_000.0,
        stop_hz: 21_450_000.0,
        name: "15m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 21_450_000.0,
        stop_hz: 21_850_000.0,
        name: "13m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 24_890_000.0,
        stop_hz: 24_990_000.0,
        name: "12m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 25_670_000.0,
        stop_hz: 26_100_000.0,
        name: "11m SW Broadcast",
        kind: BandKind::Broadcast,
    },
    // upstream types this amateur; 27 MHz CB is licence-exempt and the 10m amateur band only starts at 28 MHz
    Band {
        start_hz: 26_965_000.0,
        stop_hz: 27_405_000.0,
        name: "CB",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 28_000_000.0,
        stop_hz: 29_700_000.0,
        name: "10m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 50_000_000.0,
        stop_hz: 52_000_000.0,
        name: "6m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 70_000_000.0,
        stop_hz: 70_500_000.0,
        name: "4m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 87_500_000.0,
        stop_hz: 108_000_000.0,
        name: "FM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 108_000_000.0,
        stop_hz: 117_900_000.0,
        name: "Airband VOR/ILS",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 118_000_000.0,
        stop_hz: 137_000_000.0,
        name: "Airband Voice",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 137_000_000.0,
        stop_hz: 138_000_000.0,
        name: "Polar orbiting satellites",
        kind: BandKind::Satellite,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 146_000_000.0,
        name: "2m ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 440_000_000.0,
        name: "70cm ham band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 1_089_000_000.0,
        stop_hz: 1_091_000_000.0,
        name: "ADS-B",
        kind: BandKind::Aviation,
    },
];
