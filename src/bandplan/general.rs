//! Band plan for worldwide general use.
//!
//! Transcribed from https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/general.json, fetched 2026-10-11.
//! The 10m amateur upper edge is corrected to 29.7 MHz and CB and PMR446 are retyped as licence-exempt; see the comments on those bands.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "general",
    label: "General (worldwide)",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/general.json",
    bands: &BANDS,
};

static BANDS: [Band; 44] = [
    Band {
        start_hz: 148_500.0,
        stop_hz: 283_500.0,
        name: "Long Wave",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 526_500.0,
        stop_hz: 1_606_500.0,
        name: "Medium Wave",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 1_800_000.0,
        stop_hz: 2_000_000.0,
        name: "160m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_300_000.0,
        stop_hz: 2_468_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 3_200_000.0,
        stop_hz: 3_400_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 3_500_000.0,
        stop_hz: 3_950_000.0,
        name: "80m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_950_000.0,
        stop_hz: 4_000_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 4_750_000.0,
        stop_hz: 4_995_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 5_005_000.0,
        stop_hz: 5_060_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 5_351_500.0,
        stop_hz: 5_366_500.0,
        name: "60m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_900_000.0,
        stop_hz: 6_200_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 7_000_000.0,
        stop_hz: 7_200_000.0,
        name: "40m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 7_200_000.0,
        stop_hz: 7_450_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 9_400_000.0,
        stop_hz: 9_900_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 10_100_000.0,
        stop_hz: 10_150_000.0,
        name: "30m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 11_600_000.0,
        stop_hz: 12_100_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 13_570_000.0,
        stop_hz: 13_870_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 14_000_000.0,
        stop_hz: 14_350_000.0,
        name: "20m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 15_100_000.0,
        stop_hz: 15_800_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 17_480_000.0,
        stop_hz: 17_900_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 18_068_000.0,
        stop_hz: 18_168_000.0,
        name: "17m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 18_900_000.0,
        stop_hz: 19_020_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 21_000_000.0,
        stop_hz: 21_450_000.0,
        name: "15m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 21_450_000.0,
        stop_hz: 21_850_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 24_890_000.0,
        stop_hz: 24_990_000.0,
        name: "12m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 25_670_000.0,
        stop_hz: 26_100_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    // upstream type is "amateur"; this is a licence-exempt service, not an amateur allocation
    Band {
        start_hz: 26_960_000.0,
        stop_hz: 27_410_000.0,
        name: "CB",
        kind: BandKind::Unlicensed,
    },
    // upstream end 29.75 MHz is wrong: the amateur allocation stops at 29.7 MHz (ITU Art. 5;
    // same value in SDR++ usa.json, and thargas International BandPlan.xml starts fixed/mobile
    // at 29_700_000)
    Band {
        start_hz: 28_000_000.0,
        stop_hz: 29_700_000.0,
        name: "10m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 50_000_000.0,
        stop_hz: 54_000_000.0,
        name: "6m Ham Band",
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
        stop_hz: 118_000_000.0,
        name: "Air Band VOR/ILS",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 118_000_000.0,
        stop_hz: 137_000_000.0,
        name: "Air Band Voice",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 137_000_000.0,
        stop_hz: 138_000_000.0,
        name: "Polar Orbiting Satellites",
        kind: BandKind::Satellite,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 148_000_000.0,
        name: "2m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 156_000_000.0,
        stop_hz: 162_025_000.0,
        name: "Marine",
        kind: BandKind::Marine,
    },
    Band {
        start_hz: 222_000_000.0,
        stop_hz: 225_000_000.0,
        name: "1.25m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 225_000_000.0,
        stop_hz: 380_000_000.0,
        name: "Military Air",
        kind: BandKind::Military,
    },
    Band {
        start_hz: 240_000_000.0,
        stop_hz: 270_000_000.0,
        name: "Military Sat",
        kind: BandKind::Military,
    },
    Band {
        start_hz: 420_000_000.0,
        stop_hz: 450_000_000.0,
        name: "70cm Ham Band",
        kind: BandKind::Amateur,
    },
    // upstream type is "amateur"; this is a licence-exempt service, not an amateur allocation
    Band {
        start_hz: 446_000_000.0,
        stop_hz: 446_200_000.0,
        name: "PMR446",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 902_000_000.0,
        stop_hz: 928_000_000.0,
        name: "33cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 1_240_000_000.0,
        stop_hz: 1_300_000_000.0,
        name: "23cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_300_000_000.0,
        stop_hz: 2_310_000_000.0,
        name: "13cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_390_000_000.0,
        stop_hz: 2_450_000_000.0,
        name: "13cm Ham Band",
        kind: BandKind::Amateur,
    },
];
