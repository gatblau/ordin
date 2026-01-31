# Words of the engineering views and the fixed screens (ScreensModule.md: Engineering views, Check it,
# What Ordin remembers, Knowledge, Team knowledge, the Slack channel screen, the Jira pieces and the
# automatic-publishing line). Other parts of the screens module keep their words in their own
# `screens-<area>.ftl` file; an id is defined in exactly one file.
#
# Ids the spec lists keep the spec's text. The ids under the heading "Chosen here" are not in the spec
# (SPEC-GAP-6 of the engineering and fixed views): a label or title the spec draws but does not name.

## Engineering views: the code-checking question and the problems line (SM-SF-2, SM-SF-3)

changes-problems-none = No new problems
changes-problems-count = { $count ->
    [one] { $count } new problem
   *[other] { $count } new problems
}
changes-problems-unchecked = Not checked for new problems: some changed files had no code checking.
changes-problems-unchecked-also = Some changed files had no code checking, so there may be more.
code-checking-label = Code checking
code-checking-ask-title = Turn on code checking for { $project }?
# safety
code-checking-ask-body = Code checking runs this project's own build scripts and add-ons on your computer. Turn it on only if you trust where this project came from.
code-checking-yes = Turn on
code-checking-no = Keep off
code-checking-turn-off = Turn off
code-checking-on = Code checking is on for this project.
code-checking-off = Code checking is off for this project.
code-checking-unanswered = Code checking hasn't been turned on for this project.
project-checks-title = Check commands
project-checks-none = No checks set
project-checks-add = Add a check command
server-no-code-views = Code views aren't available for workspaces on a server yet.

## Check it (11 §5.30, UX-A63)

check-it = Check it
check-checked-by-ordin = Checked by ordin
check-checked-at = Checked { $time }, against the version in use
check-all-passed = Every check passed.

## What Ordin remembers (ADR-111, SM-SF-4 to SM-SF-6)

memory-tab-facts = Facts
memory-tab-things = Things
memory-tab-to-check = To check
memory-to-check-line = { $count ->
    [one] { $count } item waiting for your check
   *[other] { $count } items waiting for your check
}
memory-kind-happened = Something that happened
memory-kind-known = Something known
memory-kind-summary = A summary
memory-true-since = True since { $date }
memory-true-between = True from { $from } to { $until }
memory-replaced = Replaced "{ $statement }"
memory-fading = Fading — not used since { $date }
memory-keep = Keep
memory-kept = Kept. Ordin will keep using this.
memory-history = History
memory-history-title = How this fact changed
memory-history-version = { $statement } · { $when }
memory-made-from-count = { $count ->
    [one] Made from { $count } fact
   *[other] Made from { $count } facts
}
memory-forget-confirm = Forget this fact?
memory-forget-confirm-summaries = { $count ->
    [one] This also forgets { $count } summary made from it.
   *[other] This also forgets { $count } summaries made from it.
}
memory-forgotten = Forgotten. Ordin won't use this again.
memory-things-search = Find a thing
memory-thing-counts = { $facts ->
    [one] { $facts } fact
   *[other] { $facts } facts
 } · { $links ->
    [one] { $links } link
   *[other] { $links } links
 }
memory-thing-also-called = Also called { $names }
memory-explained-in = Explained in { $page }
memory-open-page = Open the page
memory-link-line = { $from } { $kind } { $to } · since { $date }
memory-link-line-ended = { $from } { $kind } { $to } · from { $since } to { $until }
memory-link-by-person = Set by you
memory-link-by-page = From the page { $page }
memory-link-correct = Correct
memory-link-correct-title = Correct this link
memory-link-correct-help = Say how these two things are related, in a few words.
memory-link-forget = Forget
memory-link-forget-confirm = Forget that { $from } { $kind } { $to }? The facts stay.
memory-thing-forget = Forget everything about this…
memory-thing-forget-confirm = Forget everything about { $thing }? This forgets { $facts ->
    [one] { $facts } fact
   *[other] { $facts } facts
 } and { $links ->
    [one] { $links } link
   *[other] { $links } links
 }.
memory-thing-forget-confirm-summaries = { $summaries ->
    [one] It also forgets { $summaries } summary made from them.
   *[other] It also forgets { $summaries } summaries made from them.
}
memory-thing-forget-verb = Forget all of it
memory-thing-forgotten = Forgotten. Ordin won't use anything about { $thing } again.
memory-thing-count-changed = Something about { $thing } changed while you were deciding. Check the numbers again.
memory-same-as = Same as…
memory-same-as-title = Which thing is { $thing } the same as?
memory-same-as-help = The two become one. { $thing } becomes another name for the one you choose.
memory-same-as-verb = Make them one
memory-same-as-done = { $merged } is now another name for { $kept }.
memory-things-empty = Ordin hasn't noted any things in this workspace yet. When it learns who or what a fact is about, they appear here.
memory-remembered-by = Remembered by { $agent }
memory-remembered-by-ordin = Remembered by Ordin
memory-used = What Ordin used
memory-used-title = What Ordin used for this answer
memory-used-gone = { $count ->
    [one] { $count } item was forgotten since.
   *[other] { $count } items were forgotten since.
}
memory-used-page = { $page } › { $section }
memory-remembered-in-run = Remembered during this run
memory-remembered-none = Nothing was remembered during this run.
memory-held-title = Written by agents, waiting for you
memory-held-intro = Ordin doesn't use these until you keep them. Anything you leave here is removed after 30 days.
memory-held-by-agent = Written by { $agent }
memory-held-by-an-agent = Written by an agent
memory-held-from-colleague = From a file a colleague shared
memory-held-about = About { $about }
memory-held-cites = Says it comes from { $source }
memory-held-names = Names
memory-held-links = Links
memory-held-link = { $from } { $kind } { $to }
memory-held-link-ended = { $from } { $kind } { $to } · no longer true
memory-held-link-sure = { $percent } sure
memory-held-needs-check = May hold personal details. If you keep it, it isn't shared with your team until you check it.
memory-held-discard = Don't keep
memory-held-kept = Kept. Ordin will use this from now on.
memory-held-discarded = Not kept. Ordin won't use this.
memory-held-empty = Nothing written by agents is waiting for you.
memory-citations-title = Sources to confirm
memory-citations-intro = An agent named the source of each fact below. Until you confirm it, the fact stays on this computer and isn't shared with your team.
memory-cited-by-agent = { $agent } says this comes from { $source } (version { $version })
memory-cited-by-an-agent = An agent says this comes from { $source } (version { $version })
memory-citation-confirm = Confirm source
memory-citation-confirmed = Confirmed. This fact can now be shared with your team.
memory-citations-empty = No sources are waiting for you to confirm.

## Automatic publishing of a linked folder (SM-SF-7)

confluence-auto-on = Your own changes here are published to Confluence automatically.
confluence-auto-off = Changes here are not published automatically. They wait in Ready to publish, because { $names } can run programs that could change these files.
confluence-auto-off-more = { $names } and { $count } more
confluence-auto-an-agent = an agent
confluence-auto-a-crew = a crew of agents
confluence-auto-how-to-turn-on = To publish automatically again, remove their access to this folder.

## Publishing a folder to Confluence (SM-PC-1, SM-PC-2)

publish-confluence-open = Publish this folder to Confluence…
publish-confluence-heading = Publish this folder to Confluence
publish-confluence-find = Find the Confluence page to publish under
publish-confluence-search = Find
publish-confluence-here = Publish here
publish-confluence-nothing = No Confluence page matches that.
publish-confluence-waiting = Waiting for you to approve publishing this folder to Confluence.
publish-confluence-ready-heading = Ready to publish
publish-confluence-ready-none = Nothing is waiting to be published.

## Knowledge: page links (ADR-104)

knowledge-link-missing = No page called { $name }
knowledge-links-in-title = Pages that link here
knowledge-links-in-none = No other page links here yet.
knowledge-links-out-title = Links on this page
knowledge-links-truncated = { $count } more links are not listed.
knowledge-other-names = Also called { $names }
knowledge-other-names-hint = Other names for this page, one per line. Links using them find this page.
knowledge-tags-hint = Tags, separated by commas
knowledge-name-in-use = "{ $name }" is already the name of another page, so it isn't used for this one.
knowledge-title-in-use = The file was given the title "{ $title }", which another page already has. This page keeps its title.
knowledge-suggestions-title = Suggestions
knowledge-suggest-link = Link "{ $words }" to { $page }?
knowledge-suggest-duplicate = This page and { $page } have nearly the same name. They may cover the same subject.
knowledge-suggest-duplicate-text = This page and { $page } say much the same. They may cover the same subject.
knowledge-renamed-links = Links on { $count } other pages were updated to the new title.
knowledge-filter-hint = Filter pages — try tag:suppliers or state:out-of-date
knowledge-not-adopted = { $count } files in the knowledge folder aren't pages
knowledge-not-adopted-limit = This workspace already keeps 500 pages.
knowledge-not-adopted-large = The file is too long to keep as a page.
knowledge-not-adopted-header = The block at the top of the file can't be read.
knowledge-not-adopted-other-workspace = It's a copy of a page that belongs to another workspace.

## Team knowledge (ADR-103, ADR-110)

team-share-with-team = Share knowledge with a team…
team-share-dialog-title = Share this workspace's knowledge with a team
team-share-dialog-pick = Choose a folder in a GitHub or GitLab repository
team-share-dialog-address = Or paste the folder's address from GitHub or GitLab
team-share-dialog-address-invalid = Paste the address of a folder as GitHub or GitLab shows it.
team-share-dialog-all = Everything Ordin remembers in this workspace and every knowledge page are shared with the people who can open this folder.
team-share-dialog-history = What you share stays in the repository's history, even after you delete it.
team-share-dialog-verb = Start sharing
team-header = Team knowledge: { $folder }
team-available = { $facts ->
    [one] { $facts } new fact
   *[other] { $facts } new facts
 } and { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
 } from { $people }
team-get = Get them
team-share = Share what we've learned
team-nothing-to-share = Nothing new to share.
team-waiting-review = Waiting for review in { $repository }
team-held-for-check = { $count ->
    [one] { $count } fact waits for you to check it
   *[other] { $count } facts wait for you to check them
}
team-check-keep = Keep it
team-not-taken-title = Not taken in
team-not-taken-file = { $file } could not be read.
team-not-taken-fact = A shared fact was not taken in: it reads like an instruction or breaks a rule.
team-shared-by = Shared by { $name }, { $date }
team-cites-their-files = Cites files on { $name }'s computer
team-folder-large = This team's folder is very large; getting it may take a while.
team-stop = Stop sharing with the team
team-stop-confirm = Stop sharing with the team? Everything stays as it is here and in the team's folder; Share and Get are no longer offered.
team-stop-verb = Stop sharing
team-busy-checking = Checking your team's knowledge…
team-busy-getting = Getting what the team has learned…
team-busy-sharing = Preparing your share…
team-also-reads = Also reads
team-read-another = Read another team's knowledge…
team-read-dialog-intro = Agents in this workspace will use this team's knowledge. Nothing from it is shared to { $folder } unless you adopt it.
team-read-verb = Start reading
team-stop-reading = Stop reading
team-stop-reading-confirm = Stop reading { $folder }? Its knowledge leaves this workspace. Anything you adopted from it stays.
team-set-by-company = Set by your company
team-folder-counts = { $facts ->
    [one] { $facts } fact
   *[other] { $facts } facts
 } and { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
 }
team-folder-no-access = You can't open { $name } yet.
team-folder-review-not-required = { $name } is set to need review, but its changes aren't reviewed.
team-folder-review-cannot-tell = { $name } is set to need review, but ordin can't see its review rules with your access.
team-workspace-large = This workspace now holds more than 100,000 facts. Searches may slow down; consider reading only the parts of a folder you need.
team-adopted-from = Adopted from { $folder }
team-changed-at-source = Changed at its source since you adopted it
team-removed-at-source = Removed at its source
team-from-folder = From { $folder }
team-read-only-page = This page belongs to another team's folder. Adopt it to change it.
team-no-own-folder = This workspace reads knowledge your company names but doesn't share to a team folder.

## Slack channel screen (ADR-102)

slack-link-kept-from = Messages from { $date }, when you linked it
slack-link-older-not-read = Some older messages weren't read.
slack-link-no-access = You're no longer in #{ $channel }, so ordin can't read it.
slack-ordin-reads = Let Ordin look at new messages
slack-ordin-reads-note = Ordin reads new messages while ordin is open and asks you before it posts.
slack-message-instruction-note = This message asks ordin to do something. It's shown as information only.

## Jira pieces (ADR-106)

jira-add-to-workspace = Jira issue…
jira-add-paste = Paste the issue's address from Jira
jira-add-search = Or search your Jira issues
jira-add-none = No issues match.
jira-add-not-connected = Connect Jira in Settings to link an issue.
handover-jira-issue-label = Which issue?
handover-jira-issue-new = A new issue will be created in { $project }
handover-jira-use-existing = Use an existing issue…
jira-item-changed = Changed in Jira, { $time }
jira-item-bring-in = Bring in the new version

## Chosen here (SPEC-GAP-6): words the spec draws but gives no id for

engineering-editor-title = Code editor
engineering-changes-title = Changes waiting
engineering-terminal-title = Terminal
engineering-settings-title = Project settings
engineering-save = Save
engineering-discard-edits = Discard my edits
engineering-entry-accept = Accept
engineering-entry-reject = Reject
engineering-keep-this-one = Keep this one
engineering-keep-yours = Keep mine
engineering-use-the-agents = Use the agent's
engineering-commit-message = Message for this commit
engineering-commit-and-push = Commit and push
engineering-check-remove = Remove
engineering-tests-passed = Tests passed
engineering-tests-failed = Tests failed
check-running = Checking…
check-fix-it = Fix it
check-ask-ordin-to-fix = Ask { $coordinator } to fix it
check-change-it-yourself = Change it yourself
check-ask-ordin-text = Check it found a problem with { $subject }: { $finding } Can you fix it?
check-asked-ordin = Sent to Ordin; the answer is in the conversation.
# The Checker's findings (Checker.md step 40, CHK-MSG-1); $part is the part's name.
check-part-invalid = { $part } doesn't pass one of ordin's checks.
check-parts-disagree = Two parts of this agent say different things about { $what }: { $part_a } and { $part_b }.
check-choose-packs = Choose which { $class } this agent uses when the workspace doesn't say.
check-format-cannot-hold = { $format } format can't hold { $what }, so it would be left out.
check-no-place-in-agent = { $what } has no place in an ordin agent.
check-whole-repository = Checked with the whole of { $repository }.
check-agent-would-not-pass = { $agent } would no longer pass its checks: { $reason }
check-trust-needs-it-approval = { $tool } runs a program on this computer that your organisation's policy hasn't approved.
check-tool-not-allowed = { $tool } isn't one of the tools your organisation allows.
check-job-not-covered = No task does the job "{ $job }".
check-name-collision = Another agent here is already called "{ $name }".
check-task-name-not-verb = The task "{ $task }" should be named for what it does, starting with a verb such as "Summarise".
check-text-too-long = { $field } is too long.
check-date-ambiguous = { $field } has a date that can be read two ways. Write it as 2026-01-31.
check-borrowed-unreachable = { $part } comes from a repository the people using this agent can't reach.
check-borrowed-apm-unsupported = { $count ->
    [one] One borrowed part
   *[other] { $count } borrowed parts
} can't be kept in the APM format.
check-bare-command-name = Write the command "{ $command }" with its agent's name in front, so it can't be mistaken for another agent's.
check-publish-gate-not-met = This agent can't be published yet: { $reason }
check-installed-part-gone = { $part } is no longer in its repository.
check-installed-part-withdrawn = { $part } has been withdrawn.
check-withdrawal-invalid = The withdrawal of { $part } can't be made as written.
check-agent-stops = { $agent } would stop working.
check-vocabulary-term-cited = "{ $term }" is still used by { $parts }.
check-vocabulary-status-fixed = "{ $term }" is one of ordin's fixed states and can't be retired.
check-lifecycle-invalid = { $part } has a status ordin doesn't recognise.
agents-made-of = What { $agent } is made of
# SM-AG-1 to SM-AG-3: remove an agent, and accept or decline its update.
agent-remove = Remove { $agent }…
agent-remove-title = Remove { $agent } from this workspace?
agent-remove-body = { $agent } will no longer work in this workspace. Other workspaces that use it keep it.
agent-remove-verb = Remove
agent-removed = { $agent } is removed from this workspace.
agent-update-heading = A new version of { $agent } is ready.
agent-update = Update
agent-update-decline = Not now
agent-update-change-unknown = Something about what it may do has changed.
may-do-tool-added = It can now use { $name }.
may-do-tool-removed = It can no longer use { $name }.
may-do-read-added = It can now read more than before.
may-do-read-removed = It can no longer read some of what it could.
may-do-connection-added = It can now connect to { $name }.
may-do-connection-removed = It can no longer connect to { $name }.
may-do-policy-changed = What it may do without asking has changed.
may-do-declaration-changed = What it says it does has changed.
may-do-authority-changed = How much it may decide has changed.
fixed-show-more = Show more
fixed-back = Back
fixed-nothing-here = Nothing here yet.
# SM-EM-1: the empty Agents and Knowledge screens say what to do.
agents-none-yet = No agents in this workspace yet. Ask Ordin to find one for the work.

## The agent library (SM-AL-1 to SM-AL-4)
library-title = Agent library
library-browse = Browse the agent library
library-from = From: { $from }
library-suggested = Ordin suggests this one
library-uses = Uses: { $uses }
library-add = Add it
library-adding = Adding { $agent }…
library-added = { $agent } is added to this workspace. Tell Ordin to go ahead in the conversation.
library-already-added = Added to this workspace.
library-unavailable = This agent can't be set up for this workspace.
library-needs-approval = This agent needs your company's approval before it can be added.
library-unsafe = This agent could not be added safely.

## Who may use which folder (SM-FA-1, SM-FA-2)
folder-access-heading = What it can use
folder-access-agents-heading = Which agents can use it
folder-access-none = Can't use
folder-access-read = Can read
folder-access-read-and-change = Can read and change
knowledge-none-yet = No pages yet. Ask Ordin to write one, or keep what it learns.
knowledge-title = Title
knowledge-body = Page
knowledge-edit = Edit
knowledge-link-it = Link it
knowledge-dismiss = Dismiss
knowledge-show-them = Show them
team-error-unreachable = Your team's knowledge can't be reached just now.
team-error-sign-in = Sign in again to reach your team's knowledge.
team-error-read-only = You can read this team's knowledge but not add to it.
slack-open-in-slack = Open in Slack
slack-remove-from-workspace = Remove from this workspace
slack-remove-confirm = Remove this channel from the workspace?
jira-add = Add
jira-open-settings = Open Settings › Jira

# ---- Catch-up of words (ScreensModule.md, New messages) ------------------------------
memory-page-gone = From a page that is no longer here
doc-glance = Changed { $date } · { $who }
doc-origin-person = saved by you
doc-origin-agent = changed by an agent
doc-origin-app = changed in { $app }
doc-open-in-app = Open in its app
# SM-PDF-1: a PDF is read in its own app; the window has no embedded viewer.
doc-pdf-opens-in-app = PDFs open in their own app. Use Open in its app to read this one.
# SM-PV-1: a preview of kind `none` (no thumbnail and no text for this kind on this computer).
doc-no-preview = There's no preview of this kind of file. It opens in its own app.
doc-no-editor = No editor for this kind of file — open it in your usual app?
doc-open-usual-app = Open in your usual app
doc-cut = Only the start of this document is shown.
doc-versions = Versions
doc-version-line = Version { $version } · { $date } · { $who }
doc-restore = Restore
doc-restore-title = Go back to version { $version }?
doc-restore-body = The document goes back to this version.
doc-published-as = Published as ({ $count })
doc-copy-kept = Kept up to date
doc-edit = Edit
doc-save = Save
doc-cancel = Cancel
doc-tool-heading = Heading
doc-tool-bold = Bold
doc-tool-italic = Italic
doc-tool-bullets = Bulleted list
doc-tool-numbers = Numbered list
doc-saved-not-sent = Changed by you — not yet sent back
doc-discard-question = Discard your changes?
doc-discard-verb = Discard changes
doc-keep-changing = Keep changing
doc-move-to-bin = Move to Bin…
doc-bin-title = Move “{ $name }” to the Bin?
doc-bin-body = It leaves this workspace and goes to this computer's Bin, where you can still get it back.
doc-bin-verb = Move to Bin
workflow-imported-band = Imported — not saved yet
workflow-save-to-workspace = Save to this workspace
workflow-discard = Discard
workflow-imported-note-text = Imported from a file
workflow-asks-for = Asks for
workflow-agents = Agents
workflow-steps = Steps
workflow-limits = Limits
workflow-versions = Versions
workflow-agent-missing = Not in this workspace
workflow-agent-other-version = Another version is installed here
workflow-version-line = Version { $version } · { $date } · { $note }
workflow-restore = Restore
workflow-run = Run…
workflow-change = Change
workflow-duplicate = Duplicate…
workflow-export = Export…
workflow-rename = Rename…
workflow-remove = Remove…
workflow-run-missing-agent = An agent this workflow uses isn't in this workspace.
workflow-rename-field = New name for the workflow
workflow-rename-verb = Rename
workflow-rename-empty = Give the workflow a name.
workflow-renamed-from = Renamed from “{ $name }”.
workflow-copy-of = Copy of { $name }
workflow-duplicate-to = Workspace
workflow-duplicate-verb = Duplicate
workflow-export-note = The file holds the steps, the agents' names and the limits. It holds no documents, links, folder access or history.
workflow-remove-title = Remove the workflow “{ $name }”?
workflow-remove-body = The workflow and its versions on this computer are removed. Its agents, its past runs in history and any copy already shared stay.
workflow-move-up = Move up
workflow-move-down = Move down
workflow-remove-step = Remove step
workflow-change-note = What changed
workflow-saved-as = Saved as version { $version }
draft-agent-title = Agent draft: { $name }
draft-band-draft = Draft — only you can see it
draft-band-waiting = Waiting for review on GitHub
draft-band-changes-asked = Changes asked for
draft-band-approved-not-signed = Approved on GitHub — not signed yet
draft-band-published = Published
draft-next = Next
draft-next-fix = { $count ->
    [one] Fix 1 check
   *[other] Fix { $count } checks
}
draft-next-rehearse = Rehearse
draft-try = Try it here — only you
draft-publish = Publish…
draft-checked-line = Fixed rules, the same answer every time. No agent decides these.
draft-passed = { $count } passed
draft-tidied = ordin tidied { $count } things in the wording
draft-what-it-is = What it is
draft-will = It will
draft-wont = It won't
draft-needs = It needs
draft-tasks = Its tasks
draft-publish-to = Publish to
draft-discard = Discard
draft-discard-title = Discard the draft “{ $name }”?
draft-discard-body = The draft and its rehearsal results are deleted. Nothing was published, so nobody else is affected.
draft-rehearsal-result = { $passed } of { $total } situations passed
draft-expectation-met = Passed
draft-expectation-not-met = Failed
draft-reviewer-opinion = Reviewing agent's opinion — from a model, so it passes or fails nothing
draft-send-change = Send the change…
draft-part-title = Part draft: { $name }
draft-part-used-by = Changing it changes { $count } agents once it's published: { $names }
draft-part-how-to-write = How to write it
draft-part-changes-what = Does this change what the part does?
draft-part-correction = No, it's a correction
draft-part-yes = Yes
repo-parts-title = Parts of { $name }
repo-parts-switch = Agent repository:
repo-tab-parts = Parts
repo-tab-words = Words
repo-tab-layout = Layout
repo-tab-health = Health
repo-apm-note = { $name } is in APM format, so ordin can withdraw parts from it but not change them.
repo-parts-search = Search the parts
repo-show-in-use = In use
repo-show-not-for-new = Not for new agents
repo-show-withdrawn = Withdrawn
repo-state-current = In use
repo-state-not-for-new = Not for new agents
repo-state-withdrawn = Withdrawn
repo-state-not-offered = Read, not offered
repo-read-it = Read it
repo-ask-about-part = Ask Ordin about this part
repo-used-by = Used by
repo-history = History of this part
repo-more = More
repo-withdraw = Withdraw…
repo-new-part = New part…
repo-health-checked = Checked { $time }, after the last update.
repo-health-unused = Not used by any agent
repo-health-look-alike = Look alike
repo-health-shared-identifier = Same name in two repositories
repo-health-broken-reference = Uses a part that can't be found
repo-health-used-deprecated = Used while not for new agents
repo-health-used-withdrawn = Used while withdrawn
repo-health-uncited-term = Words no part uses
repo-health-journey-not-offered = Lifecycles not offered
repo-health-line = Health suggests; it decides nothing.
withdraw-title = Withdraw { $name }
withdraw-used-by = These agents stop on every computer that uses them once this is published:
withdraw-reason = Why is it withdrawn?
withdraw-replace = Replace it with
withdraw-verb = Withdraw…
handover-screen-title = Handed to you: { $title }
handover-screen-from = From { $name }, { $time }
handover-screen-stage = Stage: { $stage }
handover-screen-arrived = { $done } of { $total } handed over.
handover-drafted-by-ordin = Drafted by Ordin
handover-line-instruction = This line asks an agent to do something. Agents won't follow it.
handover-results = What's handed over
handover-checked-by = Checked by { $name }'s ordin
handover-accept = Accept and start
handover-accept-question = Start a new workspace for this, or use one you have?
handover-accept-new = New workspace
handover-return = Return to { $name }…
handover-return-why = Why are you returning it?
handover-return-option = { $name } — { $stage } (round { $round })
handover-take-back = Take it back…
handover-take-back-title = Take back the handover to { $name }?
handover-take-back-verb = Take it back
handover-remind = Remind { $name }…
handover-state-draft = Draft
handover-state-sent = Waiting for { $name }
handover-state-seen = Seen by { $name }
handover-state-accepted = Accepted by { $name }
handover-state-sent-back = Returned by { $name }
handover-state-taken-back = Taken back
lifecycle-title = Lifecycle: { $name }
lifecycle-readable-note = Shows the handovers in places you can read.
lifecycle-handover-count = { $count ->
    [one] 1 handover
   *[other] { $count } handovers
}
lifecycle-you-are-here = You are here
lifecycle-stage-agent = Agent: { $agent }
lifecycle-stage-hands-over = Hands over: { $what }
lifecycle-none = This workspace doesn't follow a stage of a lifecycle.
import-title = Import an agent or a part
import-what-ordin-reads = Agents made in ordin, APM packages, and Claude Code and GitHub Copilot set-ups.
import-what = What are you bringing in?
import-agent = An agent
import-part = One part
import-part-kind = What kind of part is it?
import-which-repository = Which agent repository is it for?
import-choose-folder = Choose a folder…
import-verb = Import
installed-files-title = The installed files of { $agent }
installed-files-note = Read only. ordin wrote these from the agent; to change them, change the agent.
installed-files-show = Show the installed files
email-link-note = Ordin and the agents you allow can read these messages. Nothing is sent without you.
email-list-empty = No messages here yet.
email-cc = Cc
email-hidden-text = This message holds hidden text, which isn't shown.
email-cut = Only the start of this message is shown.
email-discard-body = The draft is deleted from this computer. Nothing was sent.
email-sent = Sent. It's in your Sent folder.
email-not-sure = Not sure this was sent. It isn't in your Sent folder, but it may still have gone. Check in your mail app.
part-page-from = From: { $name }
part-page-from-ordin = From: ordin
part-page-details = Details
part-page-used-by = Used by

# Team knowledge: adopt, describe and the recall check (ScreensModule.md, ADR-110 table)
team-item-elsewhere = { $file } is already here from another folder, so it wasn't taken in again.
team-description-not-taken = A folder description wasn't used: it reads like an instruction or may hold personal data.
team-describe = Describe this folder…
team-describe-help = A short description helps people and agents know what this folder holds. It's shared the next time you share.
team-adopt = Adopt into { $folder }…
team-adopt-title = Adopt into { $folder }
team-adopt-from = From { $folder }, shared by { $name }
team-adopt-help = You can change the text first, for example to make it a rule for everyone. It's shared the next time you share.
team-adopt-verb = Adopt
team-adopted = Adopted. It's shared the next time you share.
team-adopt-again = Adopt again
team-keep-as-is = Keep as it is
team-check-recall = Check how well ordin finds this knowledge
team-busy-checking-recall = Checking how well ordin finds this knowledge…
team-recall-result = { $folder }: finds { $percent }% of the expected facts among its first 5 answers, across { $checks } questions
team-recall-no-checks = No folder has questions to check yet.

# Part kinds in plain words (ids from AgentModel.md; text chosen here, from each id's own words)
part-kind-ability = Ability
part-kind-agent = Agent
part-kind-code-workflow = Code workflow
part-kind-group-of-abilities = Group of abilities
part-kind-hand-over = Hand-over
part-kind-language = Language
part-kind-lifecycle = Lifecycle
part-kind-memory-rules = Memory rules
part-kind-not-offered = Not offered
part-kind-reference-material = Reference material
part-kind-rehearsal-checks = Rehearsal checks
part-kind-result-must-hold = Result must hold
part-kind-role = Role
part-kind-rule = Rule
part-kind-task = Task
part-kind-tool = Tool
part-kind-way-of-working = Way of working
part-kind-words = Words
part-kind-working-mode = Working mode
part-kind-working-practice = Working practice
part-kind-writing-guide = Writing guide
part-kind-writing-style = Writing style
