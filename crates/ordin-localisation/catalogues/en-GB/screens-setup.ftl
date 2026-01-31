# Words of first start (ScreensModule.md, amendment January 2026 "First start, Add to this workspace
# and removing a link", SM-FS-1 to SM-FS-5). Each step's message is `setup-step-<kind with _ as ->` and
# each Setup Assistant result code's message is `<code lower-cased, _ as ->` (crates/ordin-setup/src/checks.rs).
# Each id is defined here only.

# ---- Getting ready ----------------------------------------------------------------------------------
setup-getting-ready = Getting you ready…
setup-progress-count = { $done } of { $total } done
setup-sends-to = To do the work, Ordin sends what it reads to { $harness }. Your documents stay where they live.

# ---- A step's state, for a screen reader --------------------------------------------------------------
setup-step-state-waiting = Not yet:
setup-step-state-running = In progress:
setup-step-state-done = Done:
setup-step-state-needs-you = Needs you:
setup-step-state-failed = Didn't work:

# ---- The steps ---------------------------------------------------------------------------------------
setup-step-company-settings = Checking your company's settings
setup-step-harness = Checking { $harness } is on this computer
setup-step-harness-sign-in = Signing you in to { $harness }
setup-step-tokens = Checking your sign-ins
setup-step-agent-repositories = Connecting the company's agent library
setup-step-agent-folders = Preparing your agents' folders
setup-step-programs = Checking the programs ordin uses
setup-step-craft-apps = Checking the company's apps
# Only in the start checks on the desktop (ScreensModule.md SM-FS-8).
setup-step-second-install = Checking for another copy of ordin

# ---- Signing in --------------------------------------------------------------------------------------
setup-sign-in = Sign in to { $harness }
setup-sign-in-note = Opens { $harness }'s sign-in page in your browser. This is the only sign-in needed now.
setup-signing-in = Waiting for you to sign in in your browser…

# ---- Actions -----------------------------------------------------------------------------------------
setup-try-again = Try again
# Claude Code is not on this computer: the person installs it and checks again (ScreensModule.md SM-FS-9).
setup-install-claude-code = Install Claude Code
setup-check-again = Check again

# ---- Harness names -----------------------------------------------------------------------------------
harness-claude-code = Claude Code
harness-copilot = GitHub Copilot

# ---- Results -----------------------------------------------------------------------------------------
setup-no-profile-address = ordin can't reach your company's settings.
setup-profile-unreachable = Your company's settings can't be reached just now. ordin goes on with the last ones it has.
setup-harness-missing = Claude Code isn't on this computer yet. Install it, then choose Check again.
setup-harness-untested = This version of Claude Code hasn't been checked with ordin yet. Install a checked version, then try again.
setup-harness-check-failed = Claude Code couldn't be checked.
setup-no-harness-access = You don't have Claude access yet.
setup-sign-in-timeout = The sign-in wasn't finished in time.
setup-sign-in-on-server = Sign in on the computer this server runs on.
setup-no-ready-harness = ordin needs Claude Code, signed in, before it can start.
setup-catalogue-failed = Can't reach the company's agent library right now.
setup-agent-folder-failed = Your agents' folders couldn't be prepared.
setup-program-refused = This computer stopped a program ordin needs. Check the computer's security settings, then try again.
setup-program-damaged = A program ordin needs is damaged.
setup-program-failed = A program ordin needs couldn't be set up.
setup-craft-app-failed = One of the company's apps couldn't be set up.
# The second-install result (ScreensModule.md SM-UP-9, SM-FS-8).
setup-second-install = ordin is also installed in { $folder }. Remove one copy, because updates reach only the copy installed for you.
