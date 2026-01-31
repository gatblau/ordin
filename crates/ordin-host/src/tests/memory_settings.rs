//! OH-MS-1 (amended January 2026; user decision; Approved by the user January 2026): Settings ›
//! Memory "Say which facts an answer used" reaches Ordin's seed.

use super::support::*;

// spec: ordin-desktop/components/OrdinHost#OH-MS-1
#[tokio::test]
async fn happy_path_the_seed_asks_ordin_to_name_the_facts_it_used_when_turned_on() {
    let f = fixture().await;
    f.memory.settings.lock().unwrap().say_which_facts = true;
    let (seed, _) = f
        .host
        .build_seed(f.ws, &crate::host::Carry::default())
        .await
        .unwrap();
    assert!(seed.contains(crate::seed::SAY_WHICH_FACTS), "{seed}");
}

#[tokio::test]
async fn edge_the_default_seed_says_nothing_about_naming_facts() {
    let f = fixture().await;
    let (seed, _) = f
        .host
        .build_seed(f.ws, &crate::host::Carry::default())
        .await
        .unwrap();
    assert!(!seed.contains(crate::seed::SAY_WHICH_FACTS));
}

#[tokio::test]
async fn edge_turning_it_off_again_takes_the_line_out_of_the_next_seed() {
    let f = fixture().await;
    f.memory.settings.lock().unwrap().say_which_facts = true;
    let carry = crate::host::Carry::default();
    assert!(f
        .host
        .build_seed(f.ws, &carry)
        .await
        .unwrap()
        .0
        .contains(crate::seed::SAY_WHICH_FACTS));
    f.memory.settings.lock().unwrap().say_which_facts = false;
    assert!(!f
        .host
        .build_seed(f.ws, &carry)
        .await
        .unwrap()
        .0
        .contains(crate::seed::SAY_WHICH_FACTS));
}
