//! Band plan for Turkey.
//!
//! Transcribed from the SDR++ band plan `turkey.json`, fetched 2026-10-11.
//! Dropped the inverted "75GHz" entry (start 75.5 GHz, end 7.6 GHz); its intended
//! edges could not be established, so nothing was substituted. Everything else is
//! verbatim, including the suspect 3cm entry flagged below.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "turkey",
    label: "Turkey",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/turkey.json",
    bands: &BANDS,
};

static BANDS: [Band; 45] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "LW",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 472_000.0,
        stop_hz: 479_000.0,
        name: "630m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 1_810_000.0,
        stop_hz: 1_850_000.0,
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
        start_hz: 5_351_500.0,
        stop_hz: 5_366_500.0,
        name: "60m",
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
        start_hz: 26_565_000.0,
        stop_hz: 27_405_000.0,
        name: "CB",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 27_750_000.0,
        stop_hz: 28_000_000.0,
        name: "Pagers",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 28_000_000.0,
        stop_hz: 29_700_000.0,
        name: "10m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 50_030_000.0,
        stop_hz: 51_000_000.0,
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
        start_hz: 108_000_000.0,
        stop_hz: 117_975_000.0,
        name: "Airband VOR/ILS",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 117_975_000.0,
        stop_hz: 137_000_000.0,
        name: "Airband Voice",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 146_000_000.0,
        name: "2m",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 167_000_000.0,
        stop_hz: 167_100_000.0,
        name: "Pagers",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 169_400_000.0,
        stop_hz: 169_475_000.0,
        name: "Sayac Okuma",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 173_882_500.0,
        stop_hz: 174_000_000.0,
        name: "Public announcement systems",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 174_000_000.0,
        stop_hz: 216_000_000.0,
        name: "DVB-T",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 216_000_000.0,
        stop_hz: 233_000_000.0,
        name: "T-DAB",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 328_600_000.0,
        stop_hz: 335_400_000.0,
        name: "ILS-Glide Path",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 380_000_000.0,
        stop_hz: 385_000_000.0,
        name: "Public Safety/Emergency",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 390_000_000.0,
        stop_hz: 395_000_000.0,
        name: "Public Safety/Emergency",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 430_200_000.0,
        stop_hz: 430_700_000.0,
        name: "70cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 431_550_000.0,
        stop_hz: 431_825_000.0,
        name: "70cm-RepeaterRX",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 432_000_000.0,
        stop_hz: 432_975_000.0,
        name: "70cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 433_400_000.0,
        stop_hz: 434_000_000.0,
        name: "70cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 435_000_000.0,
        stop_hz: 438_000_000.0,
        name: "70cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 439_150_000.0,
        stop_hz: 439_425_000.0,
        name: "70cm-RepeaterTX",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 445_250_000.0,
        stop_hz: 445_462_500.0,
        name: "Public announcement systems",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 446_006_250.0,
        stop_hz: 446_196_875.0,
        name: "PMR446",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 865_000_000.0,
        stop_hz: 868_000_000.0,
        name: "RFID",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 916_100_000.0,
        stop_hz: 918_900_000.0,
        name: "RFID",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 1_240_000_000.0,
        stop_hz: 1_300_000_000.0,
        name: "23cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 1_880_000_000.0,
        stop_hz: 1_900_000_000.0,
        name: "DECT",
        kind: BandKind::Other,
    },
    Band {
        start_hz: 5_650_000_000.0,
        stop_hz: 5_670_000_000.0,
        name: "5GHz",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_820_000_000.0,
        stop_hz: 5_850_000_000.0,
        name: "5GHz",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 24_000_000_000.0,
        stop_hz: 24_050_000_000.0,
        name: "24GHz",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 47_000_000_000.0,
        stop_hz: 47_200_000_000.0,
        name: "47GHz",
        kind: BandKind::Amateur,
    },
    // Upstream value kept verbatim: there is no amateur allocation near 104.5 GHz
    // (IARU Region 1 microwave bands run 76-81, 122, 134 and 241 GHz), so this looks
    // like a decimal-place slip for the 10.45 GHz segment -- but the intended edges
    // could not be established, so it is transcribed as published.
    Band {
        start_hz: 104_500_000_000.0,
        stop_hz: 104_520_000_000.0,
        name: "3cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 134_000_000_000.0,
        stop_hz: 142_000_000_000.0,
        name: "134GHz",
        kind: BandKind::Amateur,
    },
];
