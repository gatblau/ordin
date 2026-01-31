//! TranscriptionService.md TRX-MACH-1 to TRX-MACH-4: the recommendation follows the machine.

use std::time::{Duration, Instant};

use super::{machine_facts, recommend, recommended_model, MachineFacts, SpeechModelChoice};

// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-3
#[test]
fn scenario_the_recommendation_follows_the_machine() {
    use SpeechModelChoice::{Larger, Smaller};
    // The Examples table of the Gherkin scenario, row for row.
    let rows: [(bool, u32, u32, bool, SpeechModelChoice); 8] = [
        (true, 8, 16, false, Larger),
        (true, 8, 8, false, Smaller),
        (false, 16, 32, true, Larger),
        (false, 8, 15, true, Larger),
        (false, 4, 16, true, Smaller),
        (false, 16, 8, true, Smaller),
        (false, 16, 32, false, Smaller),
        (false, 16, 0, true, Smaller),
    ];
    for (apple_silicon, logical_processors, memory_gib, avx2, expected) in rows {
        let facts = MachineFacts {
            apple_silicon,
            logical_processors,
            memory_gib,
            avx2,
        };
        assert_eq!(recommend(&facts), expected, "{facts:?}");
    }
}

// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-2
#[test]
fn scenario_reading_the_machine_needs_nothing_from_outside() {
    // Nothing here opens a socket; the test is also run with networking blocked by the gate.
    let started = Instant::now();
    let facts = machine_facts();
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(facts.logical_processors >= 1);
    if !cfg!(target_arch = "x86_64") {
        assert!(!facts.avx2);
    }
}

// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-3
#[test]
fn the_recommendation_is_kept_for_the_process() {
    let first = recommended_model();
    assert_eq!(first, recommend(&machine_facts()));
    assert_eq!(recommended_model(), first);
}

// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-4
#[test]
fn error_unreadable_facts_recommend_the_smaller_model() {
    // The fallbacks of TRX-MACH-2: one processor, no memory read, no AVX2.
    let unreadable = MachineFacts {
        apple_silicon: false,
        logical_processors: 1,
        memory_gib: 0,
        avx2: false,
    };
    assert_eq!(recommend(&unreadable), SpeechModelChoice::Smaller);
    let apple_unreadable = MachineFacts {
        apple_silicon: true,
        ..unreadable
    };
    assert_eq!(recommend(&apple_unreadable), SpeechModelChoice::Smaller);
}
