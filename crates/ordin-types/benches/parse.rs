//! The *Performance targets* of SharedTypes.md, measured with `std::time::Instant` (no bench crate:
//! the workspace names none, and a p95 over single calls needs only a clock).
//!
//! Each target is timed call by call over many samples after a warm-up; the run prints the p50 and
//! p95 of each and exits with status 1 when a p95 is over its target.

use std::hint::black_box;
use std::time::{Duration, Instant};

use ordin_types::id::parse_v7;
use ordin_types::text::normalise_text;
use ordin_types::time::parse_rfc3339_ms_utc;

const WARM_UP: usize = 2_000;

/// p50 and p95 of `samples` timed calls of `call`.
fn measure(samples: usize, mut call: impl FnMut()) -> (Duration, Duration) {
    for _ in 0..WARM_UP {
        call();
    }
    let mut times: Vec<Duration> = (0..samples)
        .map(|_| {
            let start = Instant::now();
            call();
            start.elapsed()
        })
        .collect();
    times.sort();
    (times[samples / 2], times[samples * 95 / 100])
}

fn main() {
    let nfc_text = "Résumé des coûts, déjà validé. ".repeat(8_000 / 31 + 1);
    let nfc_text: String = nfc_text.chars().take(8_000).collect();
    let decomposed: String = nfc_text
        .chars()
        .flat_map(|c| match c {
            'é' => vec!['e', '\u{301}'],
            'û' => vec!['u', '\u{302}'],
            'à' => vec!['a', '\u{300}'],
            other => vec![other],
        })
        .collect();

    let targets: [(&str, Duration, (Duration, Duration)); 4] = [
        (
            "parse_v7",
            Duration::from_micros(1),
            measure(100_000, || {
                black_box(parse_v7(black_box("0192a1c2-3344-7556-8677-8899aabbccdd")).ok());
            }),
        ),
        (
            "parse_rfc3339_ms_utc",
            Duration::from_micros(2),
            measure(100_000, || {
                black_box(parse_rfc3339_ms_utc(black_box("2026-01-10T09:41:07.312Z")).ok());
            }),
        ),
        (
            "normalise_text (8,000 chars, already NFC)",
            Duration::from_micros(100),
            measure(5_000, || {
                black_box(normalise_text(black_box(&nfc_text)).ok());
            }),
        ),
        (
            "normalise_text (8,000 chars, decomposed)",
            Duration::from_micros(400),
            measure(5_000, || {
                black_box(normalise_text(black_box(&decomposed)).ok());
            }),
        ),
    ];

    let mut missed = false;
    for (name, target, (p50, p95)) in targets {
        let verdict = if p95 <= target { "met" } else { "MISSED" };
        missed |= p95 > target;
        println!("{name}: p50 {p50:?}, p95 {p95:?} (target p95 <= {target:?}) {verdict}");
    }
    if missed {
        std::process::exit(1);
    }
}
