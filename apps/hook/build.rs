//! On Windows (MSVC) the hook links the Visual C++ runtime statically and the Universal C runtime,
//! which is part of Windows, dynamically: the same split Tauri makes for the desktop program. The hook
//! then imports no `VCRUNTIME140*.dll`, which a PC without the Visual C++ redistributable lacks. On any
//! other target this does nothing.

fn main() {
    // spec: ordin-desktop/phase-5-playbook#PB-REL-24
    static_vcruntime::metabuild();
}
