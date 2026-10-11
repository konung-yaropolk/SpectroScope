//! Band plan for Austria.
//!
//! Transcribed from the SDR++ band plan for Austria, fetched 2026-10-11, then
//! corrected against the OeVSV tabulation of Frequenznutzungsverordnung
//! Anlage 4 (in force 2023-03-13), also fetched 2026-10-11:
//! https://oe3.oevsv.at/export/shared/.content/.galleries/pdf-Downloads/2023-03-13_FNV-Bandplan.pdf
//! The upstream file predates that ordinance -- it stops 160m at 1950 kHz and
//! 6m at 52 MHz, and lists nothing above 2322 MHz. The three stale edges are
//! corrected and the missing amateur microwave bands added, each marked below.
//! Anlage 4's 275-3000 GHz entry is omitted: it runs past this module's 300 GHz
//! sanity limit. CB, Freenet and PMR446 are typed `other` upstream and are
//! `Unlicensed` here, per the BandKind docs.

use super::{Band, BandKind, BandPlan};

pub static PLAN: BandPlan = BandPlan {
    id: "austria",
    label: "Austria",
    source: "https://raw.githubusercontent.com/AlexandreRouma/SDRPlusPlus/master/root/res/bandplans/austria.json",
    bands: &BANDS,
};

static BANDS: [Band; 33] = [
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
        stop_hz: 2_000_000.0,
        name: "160m",
        kind: BandKind::Amateur,
    }, // FNV Anlage 4 note 3 (2023-03-13): upper edge moved to 2000 kHz, was 1950
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
    }, // FNV Anlage 4 table (2023-03-13) gives 5351,5 kHz; SDR++ and a 2020 IARU R1 note said 5351,3
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
        stop_hz: 54_000_000.0,
        name: "6m",
        kind: BandKind::Amateur,
    }, // FNV Anlage 4 note 4 (2023-03-13): extended to 54 MHz until 2030-12-31
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
        start_hz: 149_025_000.0,
        stop_hz: 149_115_625.0,
        name: "Freenet",
        kind: BandKind::Unlicensed,
    }, // licence-exempt, not `other` as typed upstream
    Band {
        start_hz: 430_000_000.0,
        stop_hz: 440_000_000.0,
        name: "70cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 446_006_250.0,
        stop_hz: 446_196_875.0,
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
        start_hz: 2_304_000_000.0,
        stop_hz: 2_310_000_000.0,
        name: "13cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_320_000_000.0,
        stop_hz: 2_322_000_000.0,
        name: "13cm",
        kind: BandKind::Amateur,
    },
    Band {
        start_hz: 2_400_000_000.0,
        stop_hz: 2_450_000_000.0,
        name: "13cm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 3_400_000_000.0,
        stop_hz: 3_410_000_000.0,
        name: "9cm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 5_650_000_000.0,
        stop_hz: 5_850_000_000.0,
        name: "6cm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 10_368_000_000.0,
        stop_hz: 10_370_000_000.0,
        name: "3cm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 10_400_000_000.0,
        stop_hz: 10_500_000_000.0,
        name: "3cm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 24_000_000_000.0,
        stop_hz: 24_250_000_000.0,
        name: "1.2cm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 47_000_000_000.0,
        stop_hz: 47_200_000_000.0,
        name: "6mm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 76_000_000_000.0,
        stop_hz: 81_500_000_000.0,
        name: "4mm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 122_250_000_000.0,
        stop_hz: 123_000_000_000.0,
        name: "2.5mm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 134_000_000_000.0,
        stop_hz: 141_000_000_000.0,
        name: "2mm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
    Band {
        start_hz: 241_000_000_000.0,
        stop_hz: 250_000_000_000.0,
        name: "1.2mm",
        kind: BandKind::Amateur,
    }, // added: in FNV Anlage 4, absent from SDR++
];
