# ordin agent — cspa-431@1.0.0

Installed by ordin. Claude Code finds the agent's skills under `.claude/skills/` and its commands under `.claude/commands/` by itself; the imports below load everything else.

## System role

@.claude/rules/system-role.md

## Operating modes & rules

@.claude/rules/modes/build-mode.md
@.claude/rules/modes/plan-mode.md
@.claude/rules/tone.md

## Control loop

`control-loops-cspa` — strategy `plan-then-execute`, trigger `human-pulled`

Moves within one iteration, from each mode's *When to exit* section and the modes this loop composes:

- **Plan** → Build
- **Build** → Plan

Runtime capabilities this loop needs: `bounded-iteration`.

@.claude/rules/control-loops/cspa.md

## Overlays

@.claude/rules/overlays/language/en-GB/en-gb.md

## Context resources

@.claude/context/house-style.md
Reference material read when it is relevant, not held in every turn:
- `.claude/context/glossary.md`

## Memory policy

**project** keeps: decisions, open-questions. Never keeps: secrets. Retention: 90 days. Store: local.

## Bundled artefacts (reference; not auto-loaded)

- Capability/delivery — `.claude/ordin/Capability-delivery/`
- ToolBinding/git — `.claude/ordin/ToolBinding-git/`
