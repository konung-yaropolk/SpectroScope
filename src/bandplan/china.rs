//! Band plan for China (mainland).
//!
//! Transcribed from the SDR++ band plan `china.json`, fetched 2026-10-11.
//! Transcribed as published; no deviation from that source.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "china",
    label: "China",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/china.json",
    bands: &BANDS,
};

static BANDS: [Band; 46] = [
    Band {
        start_hz: 135_700.0,
        stop_hz: 137_800.0,
        name: "2200m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 526_500.0,
        stop_hz: 1_606_500.0,
        name: "Medium Wave Broadcast",
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
        stop_hz: 2_495_000.0,
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
        stop_hz: 3_900_000.0,
        name: "80m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_900_000.0,
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
        stop_hz: 7_350_000.0,
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
        name: "14m Ham Band",
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
        start_hz: 24_990_000.0,
        stop_hz: 25_010_000.0,
        name: "Frequency and Time Standards",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 25_670_000.0,
        stop_hz: 26_100_000.0,
        name: "Shortwave Broadcast",
        kind: BandKind::Broadcast,
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
        start_hz: 76_000_000.0,
        stop_hz: 84_000_000.0,
        name: "FM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 87_000_000.0,
        stop_hz: 108_000_000.0,
        name: "FM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 108_000_000.0,
        stop_hz: 117_975_000.0,
        name: "Air Band Radionavigation",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 117_975_000.0,
        stop_hz: 137_000_000.0,
        name: "Air Band Voice",
        kind: BandKind::Aviation,
    },
    Band {
        start_hz: 137_000_000.0,
        stop_hz: 138_000_000.0,
        name: "LEO Satellite Downlinks",
        kind: BandKind::Satellite,
    },
    Band {
        start_hz: 144_000_000.0,
        stop_hz: 148_000_000.0,
        name: "2m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 156_487_500.0,
        stop_hz: 156_562_500.0,
        name: "Marine",
        kind: BandKind::Marine,
    },
    Band {
        start_hz: 156_672_500.0,
        stop_hz: 160_975_000.0,
        name: "Marine",
        kind: BandKind::Marine,
    },
    Band {
        start_hz: 161_475_000.0,
        stop_hz: 162_050_000.0,
        name: "Marine",
        kind: BandKind::Marine,
    },
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 440_000_000.0,
        name: "70cm Ham Band",
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
    Band {
        start_hz: 3_300_000_000.0,
        stop_hz: 3_500_000_000.0,
        name: "9cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 5_650_000_000.0,
        stop_hz: 5_850_000_000.0,
        name: "5cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 10_000_000_000.0,
        stop_hz: 10_500_000_000.0,
        name: "3cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 24_000_000_000.0,
        stop_hz: 24_250_000_000.0,
        name: "1.2cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 47_000_000_000.0,
        stop_hz: 47_200_000_000.0,
        name: "6mm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 76_000_000_000.0,
        stop_hz: 81_000_000_000.0,
        name: "4mm Ham Band",
        kind: BandKind::Amateur,
    },
];
