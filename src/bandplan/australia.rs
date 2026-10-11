//! Band plan for Australia.
//!
//! Transcribed from https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/australia.json, fetched 2026-10-11.
//! Frequencies and names are verbatim; six bands typed "amateur"/"other"
//! upstream were reclassified to Unlicensed (CB, ISM and Wi-Fi) -- see the
//! comments on those bands. Amateur edges spot-checked against the WIA band
//! plan and the ACMA spectrum plan; no frequency needed changing.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "australia",
    label: "Australia",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/australia.json",
    bands: &BANDS,
};

static BANDS: [Band; 61] = [
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
        start_hz: 520_000.0,
        stop_hz: 1_710_000.0,
        name: "Medium Wave Broadcast (AM Broadcast)",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 1_800_000.0,
        stop_hz: 1_875_000.0,
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
        stop_hz: 3_700_000.0,
        name: "80m Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_776_000.0,
        stop_hz: 3_800_000.0,
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
    // Name kept verbatim and still accurate: the ACMA ruled out amateur use of
    // 5351.5-5366.5 kHz in December 2020 over Defence sharing, and no reversal
    // is on record as of 2026-10-11 (arrl.org/news/australian-radio-amateurs-
    // denied-access-to-60-meters).
    Band {
        start_hz: 5_351_500.0,
        stop_hz: 5_366_500.0,
        name: "60m Ham Band (Unavailable)",
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
    // Typed "amateur" upstream, which is simply wrong: 27 MHz and UHF CB are
    // class-licensed citizen band services (ACMA Radiocommunications (Citizen
    // Band Radio Stations) Class Licence 2015), so BandKind::Unlicensed.
    Band {
        start_hz: 26_965_000.0,
        stop_hz: 27_405_000.0,
        name: "27MHz CB",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 27_680_000.0,
        stop_hz: 27_980_000.0,
        name: "27MHz Marine Band",
        kind: BandKind::Marine,
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
        start_hz: 87_500_000.0,
        stop_hz: 108_000_000.0,
        name: "FM Broadcast",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 108_000_000.0,
        stop_hz: 117_975_000.0,
        name: "Air Band VOR/ILS",
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
        stop_hz: 162_050_000.0,
        name: "VHF Marine Band",
        kind: BandKind::Marine,
    },
    Band {
        start_hz: 174_000_000.0,
        stop_hz: 195_000_000.0,
        name: "Digital TV Broadcast (DVB-T)",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 195_000_000.0,
        stop_hz: 209_000_000.0,
        name: "Digital Radio Broadcast (DAB+)",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 209_000_000.0,
        stop_hz: 230_000_000.0,
        name: "Digital TV Broadcast (DVB-T)",
        kind: BandKind::Broadcast,
    },
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 450_000_000.0,
        name: "70cm Ham Band",
        kind: BandKind::Amateur,
    },
    // Typed "amateur" upstream, which is simply wrong: 27 MHz and UHF CB are
    // class-licensed citizen band services (ACMA Radiocommunications (Citizen
    // Band Radio Stations) Class Licence 2015), so BandKind::Unlicensed.
    Band {
        start_hz: 476_425_000.0,
        stop_hz: 477_412_500.0,
        name: "UHF CB",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 526_000_000.0,
        stop_hz: 694_000_000.0,
        name: "Digital TV Broadcast (DVB-T)",
        kind: BandKind::Broadcast,
    },
    // Typed "other" upstream; licence-exempt under the ACMA Radiocommunications
    // (Low Interference Potential Devices) Class Licence 2015, so Unlicensed.
    Band {
        start_hz: 915_000_000.0,
        stop_hz: 928_000_000.0,
        name: "ISM Band",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 1_240_000_000.0,
        stop_hz: 1_300_000_000.0,
        name: "23cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_300_000_000.0,
        stop_hz: 2_302_000_000.0,
        name: "13cm Ham Band",
        kind: BandKind::Amateur,
    },
    // Typed "other" upstream; licence-exempt under the ACMA Radiocommunications
    // (Low Interference Potential Devices) Class Licence 2015, so Unlicensed.
    Band {
        start_hz: 2_400_000_000.0,
        stop_hz: 2_483_500_000.0,
        name: "2.4GHz WiFi (ISM Band)",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 2_400_000_000.0,
        stop_hz: 2_450_000_000.0,
        name: "13cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_300_000_000.0,
        stop_hz: 3_400_000_000.0,
        name: "9cm Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 3_400_000_000.0,
        stop_hz: 3_600_000_000.0,
        name: "9cm Ham Band (Restricted)",
        kind: BandKind::Amateur,
    },
    // Typed "other" upstream; licence-exempt under the ACMA Radiocommunications
    // (Low Interference Potential Devices) Class Licence 2015, so Unlicensed.
    Band {
        start_hz: 5_150_000_000.0,
        stop_hz: 5_590_000_000.0,
        name: "5GHz WiFi (ISM Band)",
        kind: BandKind::Unlicensed,
    },
    // Typed "other" upstream; licence-exempt under the ACMA Radiocommunications
    // (Low Interference Potential Devices) Class Licence 2015, so Unlicensed.
    Band {
        start_hz: 5_650_000_000.0,
        stop_hz: 5_835_000_000.0,
        name: "5GHz WiFi (ISM Band)",
        kind: BandKind::Unlicensed,
    },
    Band {
        start_hz: 5_650_000_000.0,
        stop_hz: 5_850_000_000.0,
        name: "6cm Ham Band",
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
        name: "12mm Ham Band",
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
    Band {
        start_hz: 122_250_000_000.0,
        stop_hz: 123_000_000_000.0,
        name: "122-123GHz Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 134_000_000_000.0,
        stop_hz: 141_000_000_000.0,
        name: "134-141GHz Ham Band",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 241_000_000_000.0,
        stop_hz: 250_000_000_000.0,
        name: "241-250GHz Ham Band",
        kind: BandKind::Amateur,
    },
];
