//! Band plan for the United States.
//!
//! Transcribed from https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/usa.json, fetched 2026-10-11.
//! The FM broadcast lower edge is corrected to 88 MHz and the licence-exempt services are retyped; bands reordered by frequency. See the band comments.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "usa",
    label: "United States",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/usa.json",
    bands: &BANDS,
};

static BANDS: [Band; 57] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "2200m Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 148_500.0,
        stop_hz: 519_000.0,
        name: "Long Wave",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 472_000.0,
        stop_hz: 479_000.0,
        name: "630m Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 525_000.0,
        stop_hz: 1_705_000.0,
        name: "Medium Wave (AM Broadcast)",
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
        start_hz: 5_330_500.0,
        stop_hz: 5_406_500.0,
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
    Band {
        start_hz: 76_000_000.0,
        stop_hz: 88_000_000.0,
        name: "TV Channels 5-6",
        kind: BandKind::Broadcast,
    },
    // upstream 87.5 MHz is the ITU Region 1 edge; the US band starts at 88 MHz (47 CFR 73.201,
    // and thargas US BandPlan.xml: 88_000_000-108_000_000)
    Band {
        start_hz: 88_000_000.0,
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
    // upstream type is "amateur"; this is a licence-exempt service, not an amateur allocation
    Band {
        start_hz: 151_820_000.0,
        stop_hz: 151_940_000.0,
        name: "MURS (lower)",
        kind: BandKind::Unlicensed,
    },
    // upstream type is "amateur"; this is a licence-exempt service, not an amateur allocation
    Band {
        start_hz: 154_570_000.0,
        stop_hz: 154_600_000.0,
        name: "MURS (upper)",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 156_000_000.0,
        stop_hz: 162_025_000.0,
        name: "Marine",
        kind: BandKind::Marine,
    },
    Band {
        start_hz: 162_362_500.0,
        stop_hz: 162_587_500.0,
        name: "NOAA Weather Radio",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 174_000_000.0,
        stop_hz: 216_000_000.0,
        name: "TV Channels 7-13",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 219_000_000.0,
        stop_hz: 220_000_000.0,
        name: "1.25m Band (lower)",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 222_000_000.0,
        stop_hz: 225_000_000.0,
        name: "1.25m Band (upper)",
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
    // licence-exempt, not amateur. Kept as upstream, but PMR446 is a European allocation: in
    // the US this range is part of the 70cm amateur band above
    Band {
        start_hz: 446_000_000.0,
        stop_hz: 446_200_000.0,
        name: "PMR446",
        kind: BandKind::Unlicensed,
    },
    // upstream type is "amateur"; this is a licence-exempt service, not an amateur allocation
    Band {
        start_hz: 462_550_000.0,
        stop_hz: 462_725_000.0,
        name: "FRS",
        kind: BandKind::Unlicensed,
    },
    // upstream type is "amateur"; this is a licence-exempt service, not an amateur allocation
    Band {
        start_hz: 467_550_000.0,
        stop_hz: 467_725_000.0,
        name: "FRS - GMRS",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 470_000_000.0,
        stop_hz: 512_000_000.0,
        name: "TV Channels 14-20",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 512_000_000.0,
        stop_hz: 608_000_000.0,
        name: "TV Channels 21-36",
        kind: BandKind::Broadcast,
    },
    // stale, left as upstream: the 2020 FCC incentive-auction repack moved TV below 608 MHz and
    // thargas US BandPlan.xml lists 614-698 MHz as "Cellular Services (Old TV Ch.38-51)"
    Band {
        start_hz: 614_000_000.0,
        stop_hz: 698_000_000.0,
        name: "TV Broadcasting",
        kind: BandKind::Broadcast,
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
