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

`parity/agent-model/` holds `tree/` (a parts tree: every kind a manifest can author, in every file format, with a vocabulary for the second read), `refusals/` (manifests each refused for one reason), `formats/` (`withdrawn.yaml`, `ordin.lock`, `bundle.manifest.json` with their expected values) and `expected/` (the expected reading of `tree/` and `refusals/`, content digests included).
