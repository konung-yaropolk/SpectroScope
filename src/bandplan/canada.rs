//! Band plan for Canada.
//!
//! Transcribed from https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/canada.json, fetched 2026-10-11.
//! The FM broadcast lower edge is corrected to 88 MHz, the 10m amateur upper edge to 29.7 MHz, and two types are corrected; see the band comments.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "canada",
    label: "Canada",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/canada.json",
    bands: &BANDS,
};

static BANDS: [Band; 43] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "2200m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 472_000.0,
        stop_hz: 479_000.0,
        name: "630m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 530_000.0,
        stop_hz: 1_700_000.0,
        name: "Mediumwave Broadcast",
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
        stop_hz: 4_000_000.0,
        name: "80m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 4_750_000.0,
        stop_hz: 5_060_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 5_351_000.0,
        stop_hz: 5_366_000.0,
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
        stop_hz: 7_300_000.0,
        name: "40m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 7_300_000.0,
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
    // upstream end 29.75 MHz is wrong: the amateur allocation stops at 29.7 MHz (ITU Art. 5 /
    // ISED RBR-4; thargas International BandPlan.xml starts fixed/mobile at 29_700_000)
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
        start_hz: 54_000_000.0,
        stop_hz: 72_000_000.0,
        name: "TV Channels 2-4",
        kind: BandKind::Broadcast,
    },
    // upstream start 76 MHz swallows TV channels 5-6; Canadian FM broadcasting is 88-108 MHz
    // (thargas International BandPlan.xml: 88_000_000-108_000_000)
    Band {
        start_hz: 88_000_000.0,
        stop_hz: 108_000_000.0,
        name: "FM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 108_000_000.0,
        stop_hz: 117_000_000.0,
        name: "Air Band VOR/ILS",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 117_000_000.0,
        stop_hz: 137_000_000.0,
        name: "Air Band Voice",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 148_000_000.0,
        name: "2m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 162_400_000.0,
        stop_hz: 162_700_000.0,
        name: "Weatheradio",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 174_000_000.0,
        stop_hz: 216_000_000.0,
        name: "TV Channels 7-13",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 222_000_000.0,
        stop_hz: 225_000_000.0,
        name: "1.25m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 450_000_000.0,
        name: "70cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 470_000_000.0,
        stop_hz: 608_000_000.0,
        name: "TV Channels 14-36",
        kind: BandKind::Broadcast,
    },
    // stale, left as upstream: 698-806 MHz went to mobile in the 2014 700 MHz auction and
    // 614-698 MHz in the 2019 600 MHz auction (ISED)
    Band {
        start_hz: 614_000_000.0,
        stop_hz: 806_000_000.0,
        name: "TV Channels 38-51",
        kind: BandKind::Broadcast,
    },
    // upstream type is "broadcast"; 806-890 MHz is cellular/SMR land mobile
    Band {
        start_hz: 806_000_000.0,
        stop_hz: 890_000_000.0,
        name: "Trunked Mobile",
        kind: BandKind::Mobile,
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
        stop_hz: 2_450_000_000.0,
        name: "13cm Ham Band",
        kind: BandKind::Amateur,
    },
];
