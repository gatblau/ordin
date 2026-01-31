# Test data

Fixtures written for ordin's own tests. Each set is committed here and written from the specification that states the behaviour; a test whose fixture folder is missing fails and never skips.

| Folder | What it proves | Specification |
|---|---|---|
| `parity/agent-model/` | A parts tree covering every part kind and format reads into the same agent model | `docs/spec/ordin-desktop/components/AgentModel.md` |
| `parity/workspaces/` | Workspace profiles resolve and compile to the expected output | `docs/spec/ordin-desktop/components/ResolverAndCompiler.md` |
| `parity/checks/` | Each check family reports the expected findings on its inputs | `docs/spec/ordin-desktop/components/Checker.md` |
| `parity/installers/` | Each installer writes the expected file tree for an input | `docs/spec/ordin-desktop/components/Installers.md` |
| `conformance/rules/` | Rules engine scenarios produce the expected verdicts | `docs/spec/ordin-desktop/components/RulesEngine.md` |

Each set is filled in by the step that uses it.

`parity/format-plugins/ordin-task/` holds one task with a title and three inputs, a copy of the bundled document agent's *Compare documents* command (CatalogueService.md CAT-TF-1), which the ordin plug-in must read and write back byte for byte (FormatPlugins.md FP-TF-1).

`parity/agent-model/` holds `tree/` (a parts tree: every kind a manifest can author, in every file format, with a vocabulary for the second read), `refusals/` (manifests each refused for one reason), `formats/` (`withdrawn.yaml`, `ordin.lock`, `bundle.manifest.json` with their expected values) and `expected/` (the expected reading of `tree/` and `refusals/`, content digests included).

`parity/workspaces/` holds `catalogue/` (a parts tree in the ordin format with its `withdrawn.yaml`: every WorkspaceProfile the cases build, with the packs, overlays and constituents they use) and `cases/` (one folder per case: `request.json`, the choices given to `build`, and `expected/`, either `error.json` or `plan.json`, `ordin.lock`, `bundle.manifest.json` and `files.json`). Every WorkspaceProfile in the catalogue must have at least one case.

`parity/installers/` holds `catalogue/` (a parts tree in the ordin format: every WorkspaceProfile the cases build, with the parts, packs and overlays they use) and `cases/` (one folder per case: `case.json`, naming the root, the harness the bundle is built for and the installer that renders it, and `expected/`, either `error.json`, the refusal, or `tree/` and `outcome.json`). `tree/` holds every file the installer renders except `ordin.lock`, which is the bundle's own lock file byte for byte; `outcome.json` holds the commands, launch paths and reference-only parts. Every installer has at least one tree case and the set holds at least one refusal. The trees are written from Installers.md, not produced by running the crate, and follow the layout Installers.md states, including the differences it names. Its own `.gitignore` keeps the `.claude/` folders of the Claude Code trees, which the root `.gitignore` excludes.

`parity/checks/` holds `catalogue/` (a clean parts tree in the ordin format: the vocabularies, a system role and a rule that every case shares) and one folder per check family Checker.md names: `authoringcheck/` (stage one, the lifecycle checks, the cross-checks, references and ingest rules over a repository read as a whole), `compositioncheck/` (one library WorkspaceProfile checked with its closure, the trust policy and the conflicts between its parts) and `artefactbuild/` (the Release Builder's package check). Each family holds `cases/`, one folder per case: `tree/`, the parts the case adds to the catalogue; `case.json`, naming the root for `compositioncheck/` and `artefactbuild/`; and `expected.json`, either the finding codes (`codes`, compared as a multiset) or, for `artefactbuild/`, each package finding's `stop`, `file` and `entry` in order. The expected findings are written from Checker.md, not produced by running the crate; `crates/ordin-checker/tests/parity.rs` reads them and fails when a folder is missing or empty.
