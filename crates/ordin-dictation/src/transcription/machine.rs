//! The speech model chosen by the machine (TranscriptionService.md TRX-MACH-1 to TRX-MACH-4): what
//! the computer is, read without the network and without a permission prompt, and the rule that
//! turns it into a recommendation. An unreadable value falls to the value that recommends
//! `Smaller`, so the facts never fail (TRX-MACH-4).

use std::sync::OnceLock;

use super::types::SpeechModelChoice;

/// Memory, in whole GiB, from which the larger model is recommended. 15, not 16: a 16 GB computer
/// reports slightly less than 16 GiB of physical memory.
const LARGER_MIN_MEMORY_GIB: u32 = 15;
/// Logical processors from which an x86 computer with AVX2 gets the larger model.
const LARGER_MIN_PROCESSORS: u32 = 8;
const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;

/// What the computer is.
// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineFacts {
    pub apple_silicon: bool,
    pub logical_processors: u32,
    pub memory_gib: u32,
    pub avx2: bool,
}

/// Reads the facts of this computer; never fails.
// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-2
pub fn machine_facts() -> MachineFacts {
    let logical_processors = std::thread::available_parallelism()
        .ok()
        .and_then(|n| u32::try_from(n.get()).ok())
        .unwrap_or(1);
    let memory_gib = total_memory_bytes()
        .map(|bytes| u32::try_from(bytes / BYTES_PER_GIB).unwrap_or(u32::MAX))
        .unwrap_or(0);
    MachineFacts {
        apple_silicon: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        logical_processors,
        memory_gib,
        avx2: avx2(),
    }
}

/// The rule: `Larger` on Apple silicon with at least 15 GiB, or on an x86 computer with AVX2, at
/// least 8 logical processors and at least 15 GiB; otherwise `Smaller`.
// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-3
pub fn recommend(facts: &MachineFacts) -> SpeechModelChoice {
    let memory = facts.memory_gib >= LARGER_MIN_MEMORY_GIB;
    let apple = facts.apple_silicon && memory;
    let x86 = facts.avx2 && facts.logical_processors >= LARGER_MIN_PROCESSORS && memory;
    if apple || x86 {
        SpeechModelChoice::Larger
    } else {
        SpeechModelChoice::Smaller
    }
}

/// `recommend(&machine_facts())`, computed once per process and kept; logged once.
// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-1
// spec: ordin-desktop/components/TranscriptionService#TRX-MACH-3
pub fn recommended_model() -> SpeechModelChoice {
    static RECOMMENDED: OnceLock<SpeechModelChoice> = OnceLock::new();
    *RECOMMENDED.get_or_init(|| {
        let facts = machine_facts();
        let choice = recommend(&facts);
        tracing::info!(
            event = "transcription.model_recommended",
            choice = choice.as_str(),
            apple_silicon = facts.apple_silicon,
            logical_processors = facts.logical_processors,
            memory_gib = facts.memory_gib,
            avx2 = facts.avx2,
            "speech model recommended"
        );
        choice
    })
}

#[cfg(target_arch = "x86_64")]
fn avx2() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn avx2() -> bool {
    false
}

/// Total physical memory through `sysctlbyname("hw.memsize")`.
#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: the name is a NUL-terminated literal; `bytes` is a writable u64 and `size` holds
    // its length, which is what `hw.memsize` writes; no new value is set.
    #[allow(unsafe_code)]
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            std::ptr::addr_of_mut!(bytes).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && size == std::mem::size_of::<u64>()).then_some(bytes)
}

/// Total physical memory through `GlobalMemoryStatusEx` (`ullTotalPhys`).
#[cfg(windows)]
fn total_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: an all-zero MEMORYSTATUSEX is a valid value of this plain C struct.
    #[allow(unsafe_code)]
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?;
    // SAFETY: `status` is writable and its `dwLength` is set, as the function requires.
    #[allow(unsafe_code)]
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some(status.ullTotalPhys)
}

/// Other systems (the server program is out of scope): unreadable, which recommends `Smaller`.
#[cfg(not(any(target_os = "macos", windows)))]
fn total_memory_bytes() -> Option<u64> {
    None
}
