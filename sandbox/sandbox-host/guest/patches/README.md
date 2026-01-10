# Patched dependencies

- `rustpython-derive-impl/` — the published `rustpython-derive-impl` 0.5.0, unchanged except
  four places that iterated a `HashMap` or `HashSet` while generating output, which now sort
  first: `src/pymodule.rs` (a function's alias names),
  `src/compile_bytecode.rs::impl_py_freeze` (frozen standard-library modules, by name) and
  the two `to_tokens` of `GetSetNursery` and `MemberNursery` in `src/pyclass.rs` (class
  properties and members, by name). Without them the guest artefact differs on every build,
  because Rust seeds `HashMap` order randomly per process (sandbox HLD OQ-WD-2). Each change
  is marked `sandboxd patch (OQ-WD-2)`. Wired in through
  `[patch.crates-io]` in `../Cargo.toml`. Remove it once an upstream release sorts the modules.
- `rustpython-vm/` — the published `rustpython-vm` 0.5.0, unchanged except `build.rs`, which
  no longer writes the build machine's environment variables into `env_vars.rs` (exposed to
  guest code through `sysconfig`). Upstream embeds every variable, so the shipped guest
  carried the builder's paths, user name and any token in the environment, and differed per
  machine and per shell (sandbox HLD OQ-WD-2). Marked `sandboxd patch (OQ-WD-2)`.
