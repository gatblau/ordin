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
memory-scope-workspace = This workspace
memory-scope-everywhere = All workspaces
memory-out-of-date = may be out of date
memory-from-conversation = From our conversation
memory-from-agent = From { $agent }
memory-from-an-agent = From an agent
memory-from-document = From { $source }
memory-from-page = From a knowledge page
memory-from-team = From your team
memory-from-you = Written by you
memory-correct = Correct
memory-correct-label = Correct this fact
memory-correct-save = Save
memory-correct-cancel = Cancel
memory-corrected = Corrected. Ordin will use the new wording.
memory-everywhere = Remember everywhere
memory-everywhere-done = Remembered in every workspace.
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
memory-citation-select = Select “{ $statement }”
memory-citations-select-all = Select all
memory-citations-select-none = Select none
memory-citations-confirm-selected = Confirm selected ({ $count })
memory-citations-confirmed-count = { $count } sources confirmed. These facts can now be shared with your team.

## Automatic publishing of a linked folder (SM-SF-7)

confluence-auto-on = Your own changes here are published to Confluence automatically.
confluence-auto-off = Changes here are not published automatically. They wait in Ready to publish, because { $names } can run programs that could change these files.
confluence-auto-off-more = { $names } and { $count } more
confluence-auto-an-agent = an agent
confluence-auto-a-crew = a crew of agents
confluence-auto-how-to-turn-on = To publish automatically again, remove their access to this folder.

## Publishing a folder to Confluence (SM-FP-1, SM-PC-2)

publish-confluence-find = Find the Confluence page to publish under
publish-confluence-search = Find
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

team-share-dialog-pick = Choose a folder in a GitHub or GitLab repository
team-share-dialog-address = Or paste the folder's address from GitHub or GitLab
team-share-dialog-address-invalid = Paste the address of a folder as GitHub or GitLab shows it.
team-share-dialog-history = What you share stays in the repository's history, even after you delete it.
team-header = Team knowledge: { $folder }
team-available = { $facts ->
    [one] { $facts } new fact
   *[other] { $facts } new facts
 } and { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
 } from { $people }
team-get = Get your team's memory and knowledge pages
team-share = Share Ordin's memory and knowledge pages with your team
team-share-what = Facts, kinds of fact, the links between them, where each fact came from, and knowledge pages. Your conversations and your documents themselves stay on this computer.
team-panel-label = Your team
team-share-outcome = Your share of { $facts ->
    [one] { $facts } fact
   *[other] { $facts } facts
 } and { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
 } is waiting for your approval in Needs you.
team-get-outcome = Got { $facts ->
    [one] { $facts } fact
   *[other] { $facts } facts
 } and { $pages ->
    [one] { $pages } page
   *[other] { $pages } pages
 } from your team.
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

## A team workspace (ScreensModule.md SM-TW-1 to SM-TW-12, ADR-121)

team-ws-share-whole = Share the whole workspace
team-ws-share-whole-line = Only this workspace's knowledge is shared with your team.
team-ws-shared-with = Shared with everyone who can open { $folder }.
team-ws-share-title = Share { $workspace } as a team workspace
team-ws-results-label = Where results are saved
team-ws-results-team-folder = In the team's folder
team-ws-preview-places = Places everyone will link, each with their own sign-in: { $places }
team-ws-preview-no-places = No places are linked yet.
team-ws-preview-stays = Stays on this computer: { $names }
team-ws-preview-agents = Agents
team-ws-agent-access = { $agent }, which can { $access }
team-ws-preview-knowledge = { $facts ->
    [one] 1 fact
   *[other] { $facts } facts
 } and { $pages ->
    [one] 1 page
   *[other] { $pages } pages
 }
team-ws-preview-results = Results will be saved in { $place }
team-ws-share-intro = Everyone who can open this folder can open this workspace with the same places and agents, see its results and see what each person did. Your conversations stay yours.
team-ws-share-verb = Share
team-ws-open-title = Open a team workspace
team-ws-open-address = Paste the address of the team's folder from GitHub or GitLab
team-ws-open-continue = Continue
team-ws-open-name = Name on this computer
team-ws-open-name-in-use = Another workspace already has this name. Choose another.
team-ws-reach-no-access = You don't have access
team-ws-reach-sign-in = Sign in to link this
team-ws-reach-not-set-up = Not set up on this computer
team-ws-reach-cannot-tell = Can't check just now; ordin tries again later
team-ws-agent-already-here = Already on this computer
team-ws-agent-not-found = Not in your agent library, so it won't be added
team-ws-open-intro = Opening links these places with your own sign-ins and gives these agents the access shown. Your conversations stay yours.
team-ws-open-verb = Open
team-ws-busy-opening = Opening the team workspace…
team-ws-header = Team workspace: { $folder }
team-ws-proposed = Waiting for your approval to share
team-ws-place-no-access = { $place }: you don't have access
team-ws-place-removal-waits = { $place } was removed by your team; it stays until your changes in it are sent.
team-ws-place-waiting = { $place } can't be reached just now; ordin tries again later.
team-ws-agent-not-here = { $agent } isn't on this computer.
team-ws-results-in = Results are saved in { $place }
team-ws-change-results = Change…
team-ws-results-here-only = Your results stay on this computer: you can read this team workspace but not add to it.
team-ws-unsent-results = { $count ->
    [one] 1 result waits to be shared
   *[other] { $count } results wait to be shared
 }
team-ws-grants-waiting = { $count ->
    [one] 1 change by your team waits for you
   *[other] { $count } changes by your team wait for you
 }
team-ws-grants-review = Review
team-ws-grant-new-agent = { $person } added { $agent }, which can { $access } — Allow?
team-ws-grant-more-access = { $person } gave { $agent } more access: it can { $access } — Allow?
team-ws-access-change = read and change { $places }
team-ws-access-read = read { $places }
team-ws-access-and = { $first }, and { $second }
team-ws-grant-left-out = Left out because you can't open them: { $places }
team-ws-grant-allow = Allow
team-ws-grant-decline = Don't allow
team-ws-grant-allowed = Allowed. { $agent } can use what your team gave it.
team-ws-grant-declined = Not allowed. Nothing changed on this computer.
team-ws-activity-run = { $person } ran { $task } with { $agent }
team-ws-activity-you = You
team-ws-activity-untitled = a task
team-ws-outcome-completed = Done
team-ws-outcome-failed = Didn't finish
team-ws-outcome-stopped = Stopped
team-ws-result-not-here = Not on this computer yet. Get your team's memory and knowledge pages to bring it here.
team-ws-activity-replaced = { $person }'s change to { $subject } was replaced by { $other }'s later share
team-ws-activity-not-shared = Not shared yet
team-ws-someone = Someone
team-ws-says-it-was = (says it was { $person })
team-ws-activity-more = Show earlier
team-ws-activity-empty = Nothing done here yet. Tasks your team runs in this workspace appear here.
team-ws-leave = Leave the team workspace
team-ws-leave-title = Leave { $workspace }?
team-ws-leave-body = It stays on this computer as your own workspace, with everything in it. You stop sharing with the team and getting what they share. Nothing changes in the team's folder.
team-ws-leave-verb = Leave
team-ws-left = You left the team workspace. Everything is still here.
team-ws-error-folder-taken = This folder already holds a team workspace. Open it with Open a team workspace… instead.
team-ws-problem-results-no-access = You can't open the place where the team saves results, so your results stay on this computer.
team-ws-problem-result-too-large = { $file } is too large to share; it stays on this computer.
team-ws-problem-result-kept = A colleague saved a result with the same name, so both are kept; yours is now { $file }.
team-ws-problem-result-not-taken = A colleague's { $file } wasn't brought here because yours has the same name. Rename yours, then get your team's memory and knowledge pages again.
team-ws-problem-description = This team workspace's settings can't be read, so your team's latest changes to it weren't taken in.
team-ws-problem-format-newer = This team workspace needs a newer ordin to take in your team's latest changes.
team-ws-problem-name-in-use = Your team renamed this workspace, but another workspace here already has that name.

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
check-task-form-invalid = The task "{ $task }" can't show its form: its fields or the text that uses them don't match.
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
# The page's kind and one-sentence summary (Open Knowledge Format `type` and `description`).
knowledge-type = Kind of page
knowledge-type-explanation = Explanation
knowledge-type-how-to = How-to
knowledge-type-decision = Decision
knowledge-type-reference = Reference
knowledge-type-glossary = Glossary
knowledge-description = Summary in one sentence
knowledge-description-hint = Shown in lists and search results. Up to 200 characters.
knowledge-details-show = Show the page's details
knowledge-details-hide = Hide the page's details
knowledge-edit = Edit
knowledge-remove = Remove…
knowledge-remove-title = Remove “{ $title }”?
knowledge-remove-body = The page leaves this workspace and goes to this computer's Bin, where you can still get it back.
knowledge-remove-verb = Remove
knowledge-removed = Removed “{ $title }”.
# A knowledge page's ⋯ menu: Export (ScreensModule.md SM-KN-3; amended January 2026; user decision; Approved by
# the user January 2026). The file is saved on this computer where the person chooses.
knowledge-export-pdf = Export as PDF…
knowledge-export-docx = Export as Word…
knowledge-exported = “{ $title }” was saved.
knowledge-undo-remove = Undo
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
# SM-PDF-1 (amended January 2026, user decision): a PDF is drawn in the page, page by page.
doc-pdf-loading = Opening the PDF…
doc-pdf-failed = This PDF can't be shown here. Open it in its app from its ⋯ menu.
doc-pdf-page = Page { $page } of { $total }
doc-pdf-zoom-in = Zoom in
doc-pdf-zoom-out = Zoom out
doc-pdf-zoom = { $percent }%
doc-pdf-more-pages = Only the first { $shown } of { $total } pages are shown.
# SM-NS-4 (amended January 2026, user decision): where the document lives, under its name.
doc-location = In { $location }
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
# The what-you-see editor used wherever a person writes formatted text (SM-NS-4.6; UX guidelines, Writing).
editor-toolbar = Formatting for { $name }
editor-heading = Heading
editor-bold = Bold
editor-italic = Italic
editor-bullets = Bulleted list
editor-numbers = Numbered list
editor-link = Link
editor-quote = Quote
editor-table = Table
editor-undo = Undo
editor-redo = Redo
editor-placeholder = Start writing…
editor-kept = Kept as it was written — this part can't be changed here
editor-link-address = Link address
editor-link-apply = Apply
editor-link-remove = Remove link
editor-link-cancel = Cancel
editor-link-invalid = Use a web address (https://…) or an email address.
# A toolbar button's tooltip with its shortcut: "Bold (⌘B)" on a Mac, "Bold (Ctrl+B)" elsewhere.
editor-tip = { $name } ({ $keys })
editor-heading-normal = Normal text
editor-heading-1 = Heading 1
editor-heading-2 = Heading 2
editor-heading-3 = Heading 3
# Leaving the editor (amended January 2026, user decision): Done saves and goes back to reading; Cancel goes
# back without saving, asking first when something was changed. Escape is Cancel.
edit-done = Done
edit-cancel = Cancel
edit-discard-title = Discard your changes?
edit-discard-verb = Discard
edit-keep-editing = Keep editing
doc-saved-not-sent = Changed by you — not yet sent back
doc-discard-question = Discard your changes?
doc-discard-verb = Discard changes
doc-keep-changing = Keep changing
doc-move-to-bin = Move to Bin…
doc-bin-title = Move “{ $name }” to the Bin?
doc-bin-body = The file goes to your computer's own Bin (or Trash), not to the Bin in this app, which lists workspaces only. You can undo it straight afterwards in the tree.
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
installed-files-title = What { $agent } is made of
installed-files-note = Read only. ordin wrote these from the agent; to change them, change the agent.
installed-files-tree = The files of { $agent }
installed-files-choose = Choose a file to read it.
installed-files-open-all = Open all folders
installed-files-close-all = Close all folders
installed-files-copy = Copy the text
installed-files-copied = Copied
installed-files-cut = Only the start of this file is shown.
installed-files-details = Details
installed-files-show = Show the installed files
installed-files-hide-tree = Hide the files
installed-files-show-tree = Show the files
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

## Facts as a thing, a word and a value; words; conflicts; what Ordin read; the memory diagram
## (ScreensModule.md SM-SF-13 to SM-SF-24).
memory-value-yes = yes
memory-value-no = no
memory-older-note = Older note
memory-older-note-help = Kept before Ordin wrote each fact as a thing, a kind of fact and a value. Ordin still uses it.
# Grouping, search, filters and topics (ScreensModule.md SM-SF-27; amended January 2026 (user decision; Approved by the user January 2026)).
memory-group-by = Group by
memory-group-by-thing = Thing
memory-group-by-kind = Kind of fact
memory-group-by-source = Source
memory-group-by-added-by = Added by
memory-group-by-topic = Topic
memory-group-title = { $name } ({ $count })
memory-group-no-thing = Not about one thing
memory-group-no-topic = No topic
memory-group-show-more = Show { $count } more
memory-groups-show-more = Show more groups
memory-added-by-you = You
memory-added-by-ordin = Ordin
memory-added-by-team = Your team
memory-added-by-an-agent = An agent
memory-facts-search = Search facts
memory-facts-search-loaded = Searching the { $count } facts loaded so far. Load more facts to search further.
memory-facts-none-match = No facts match.
memory-facts-load-more = Load more facts
memory-filters = Show only
memory-filter-conflicts = Has conflicts
memory-filter-out-of-date = Out of date
memory-filter-older = Older notes
memory-filter-workspace = This workspace
memory-filter-everywhere = Everywhere
memory-filter-topic = Topic
memory-filter-topic-all = All topics
memory-topic = Topic: { $topic }
memory-topic-none = No topic yet
memory-topic-change = Change topic…
memory-topic-title = Topic of { $thing }
memory-topic-help = A short plain label, such as Strategy, People, Funding or Suppliers.
memory-topic-new = A new topic
memory-topic-clear = No topic
memory-topic-save = Save
memory-topic-saved = Topic saved.
memory-conflicts-with = Conflicts with:
memory-keep-this = Keep this one
memory-keep-this-title = Keep this fact?
memory-keep-this-body = Ordin will stop using “{ $other }”. You can still see it in this fact's history.
memory-keep-this-verb = Keep this one
memory-keep-this-done = Kept. Ordin no longer uses the other fact.
memory-correct-value-label = New value for “{ $word }”
memory-correct-date-hint = A year, a month (2026-04) or a day (2026-04-15)
memory-tab-words = Kinds of fact
memory-words-intro = Each one is a kind of fact Ordin can keep, for example price, supplies or works for, as in “Kestrel · supplies · packaging”. Ordin and your agents add a new kind when none fits.
memory-word-object-thing = links to a thing
memory-word-object-text = links to text
memory-word-object-number = links to a number
memory-word-object-money = links to an amount of money
memory-word-object-date = links to a date
memory-word-object-yes-no = links to yes or no
memory-word-one = one at a time
memory-word-several = can have several
memory-word-by-seed = Built in
memory-word-by-ordin = Added by Ordin
memory-word-by-agent = Added by { $agent }
memory-word-by-an-agent = Added by an agent
memory-word-by-hosted-agent = Added by your organisation's agent
memory-word-by-team = From your team
memory-word-by-you = Added by you
memory-word-count = { $count ->
    [one] Used by 1 fact
   *[other] Used by { $count } facts
}
memory-word-rename = Rename
memory-word-renamed = Renamed. Facts of this kind read the new name.
memory-word-merge = Merge into…
memory-word-merge-title = Merge into another kind of fact
memory-word-merge-body = { $count ->
    [one] The 1 fact of this kind will become the kind you choose.
   *[other] The { $count } facts of this kind will become the kind you choose.
}
memory-word-merge-verb = Merge
# SM-SF-26 (amended January 2026 (user decision)): under a word whose label names a kind of note.
memory-word-not-a-kind = Not a kind of fact. Use Merge into… to move these facts to a better kind.
memory-word-merged = { $count ->
    [one] Merged. 1 fact is now of the other kind.
   *[other] Merged. { $count } facts are now of the other kind.
}
memory-read-title = What Ordin read
memory-read-version = From version { $version }
memory-read-empty = Ordin has not read this page yet.
memory-summaries-title = What Ordin summed up
memory-diagram-show = Show as diagram
memory-diagram-title = Diagram of what Ordin remembers
memory-diagram-intro = Each box is something Ordin knows about. Each arrow is a fact that joins two of them. Press an arrow to see where Ordin learnt it.
memory-diagram-left-out = Showing { $shown } of { $total } things — search to centre on one
memory-diagram-conflicts = { $word } (Conflicts)
memory-diagram-facts-title = Facts about { $thing }
memory-diagram-centred = Centred on { $thing }
memory-diagram-things-title = Things in the diagram
memory-diagram-open-thing = Open { $thing }
memory-diagram-back = Back to the list
memory-diagram-fact-title = The fact you pressed
memory-diagram-fact-close = Close
memory-diagram-retry = Try again
memory-diagram-select-hint = Press a box or an arrow in the diagram to see its facts here.
memory-diagram-not-drawn = The diagram could not be drawn. The lists show the same facts.
memory-fact-more = Details
memory-fact-less = Hide details
memory-fact-conflict-mark = Conflicts with another fact
## The 26 seed words (MemoryService.md, the vocabulary's seed), drawn while a place has not renamed them.
memory-term-works-for = works for
memory-term-works-for-meaning = An organisation the subject works for.
memory-term-member-of = member of
memory-term-member-of-meaning = A group or organisation the subject belongs to.
memory-term-owns = owns
memory-term-owns-meaning = Something the subject owns.
memory-term-location = located in
memory-term-location-meaning = A place where the subject is, or happens.
memory-term-author = written by
memory-term-author-meaning = The person or organisation that wrote the subject.
memory-term-start-date = starts on
memory-term-start-date-meaning = The date the subject starts or started.
memory-term-end-date = ends on
memory-term-end-date-meaning = The date the subject ends or ended.
memory-term-price = price
memory-term-price-meaning = What the subject costs.
memory-term-amount = amount
memory-term-amount-meaning = An amount of money the subject is for, such as a grant or a payment.
memory-term-number-of-employees = number of employees
memory-term-number-of-employees-meaning = How many people the subject employs.
memory-term-job-title = job title
memory-term-job-title-meaning = The subject's job title.
memory-term-parent-organization = part of organisation
memory-term-parent-organization-meaning = The larger organisation the subject belongs to.
memory-term-sub-organization = includes organisation
memory-term-sub-organization-meaning = An organisation that is part of the subject.
memory-term-is-part-of = part of
memory-term-is-part-of-meaning = Something the subject is a part of.
memory-term-has-part = has part
memory-term-has-part-meaning = Something that is a part of the subject.
memory-term-about = is about
memory-term-about-meaning = What the subject, such as a document, is about.
memory-term-founding-date = founded on
memory-term-founding-date-meaning = The date the subject was founded.
memory-term-founder = founded by
memory-term-founder-meaning = A person or organisation that founded the subject.
memory-term-employee = employs
memory-term-employee-meaning = A person the subject employs.
memory-term-member = has member
memory-term-member-meaning = A person or organisation that belongs to the subject.
memory-term-url = web address
memory-term-url-meaning = The subject's web address.
memory-term-legal-name = legal name
memory-term-legal-name-meaning = The subject's registered name.
memory-term-knows-about = knows about
memory-term-knows-about-meaning = A topic the subject knows about.
memory-term-manufacturer = made by
memory-term-manufacturer-meaning = The organisation that makes the subject.
memory-term-brand = brand
memory-term-brand-meaning = A brand the subject carries.
memory-term-prefers = prefers
memory-term-prefers-meaning = Something the subject, usually the person, prefers.

## Publish to… in a document's menu (amended January 2026; user decision; SM-PUB-1, SM-PUB-2)

doc-menu-open = Open
publish-to = Publish to…
publish-to-destination = { $destination }…
publish-to-title = Publish { $name } to { $destination }
publish-to-body = Choose where in { $destination } it goes. You approve it before anything is published.
publish-to-find = Find the place in { $destination } to publish under
publish-to-nothing = Nothing in { $destination } matches that.
publish-to-choose = Choose a place in { $destination } first.
publish-to-verb = Publish
publish-to-sent = { $name } waits for your approval in Needs you before it is published.

## The Documents overview (amended January 2026; user decision; SM-DS-4 to SM-DS-6)

documents-recent-heading = Recently changed
documents-search-label = Search this workspace's documents
documents-search-none = No document matches that.
documents-recent-none = No documents have changed yet.
documents-row-changed = { $place } · changed { $date }
documents-row-changed-by = { $place } · changed { $date } by { $who }

## The page of one agent (amended January 2026; user decision; SM-AG-6 to SM-AG-8)

agents-choose-in-tree = Choose an agent under Agents in the side tree to see what it does.
agent-page-what = What it does
agent-page-tasks = Recent tasks
agent-page-no-tasks = It hasn't done any tasks in this workspace yet.
agent-page-task-line = { $date } · { $state }
agent-page-choices = Its choices
agent-page-no-choices = It has no choices to make.
agent-page-choice-line = { $kind }: { $label }
agent-page-choice-fixed = { $kind }: { $label } (set by its maker)
agent-page-unknown = This agent is no longer in this workspace.

## Dashboards (amended January 2026; user decision; SM-DB-1 to SM-DB-4)

dashboard-keep = Keep as dashboard…
dashboard-keep-title = Keep { $title } as a dashboard
dashboard-keep-body = It goes under Dashboards in this workspace. Refresh makes it again from the same documents.
dashboard-keep-name = Name
dashboard-keep-verb = Keep
dashboard-kept = { $name } is now under Dashboards.
dashboard-keep-not-found = Ordin can't find where this report came from any more, so it can't be kept.
dashboard-keep-from-crew = This report came from a crew. Keep it from the crew's workflow instead.
dashboard-keep-name-needed = Give the dashboard a name of up to 80 characters.
dashboard-refresh = Refresh
dashboard-refresh-started = { $name } is being made again. It shows here when it's ready.
dashboard-kept-on = Kept { $date }
dashboard-as-of = As of { $time }
dashboard-none-yet = No report has been made for this dashboard yet.
dashboard-gone = This dashboard is no longer in this workspace.

## A dashboard's menu: Refresh, Export and Remove (amended January 2026; user decision; Approved by the
## user January 2026; SM-DB-5 to SM-DB-8)

dashboard-export = Export
dashboard-export-pdf = PDF…
dashboard-export-pptx = PowerPoint…
dashboard-export-xlsx = Excel…
dashboard-export-docx = Word…
dashboard-add-to-presentation = Add to an existing PowerPoint…
dashboard-exported = { $name } was saved.
dashboard-added-to-presentation = { $name } was added to the end of the presentation.
dashboard-added-as-copy = The presentation was open or couldn't be changed, so a copy with { $name } added was saved instead.
dashboard-remove = Remove…
dashboard-remove-title = Remove { $name }?
dashboard-remove-body = It goes from Dashboards in this workspace. The reports it showed are not deleted.
dashboard-remove-verb = Remove
dashboard-removed = { $name } was removed.
# Words inside a saved dashboard file, and the file chooser's title (OrdinDesktop.md DSK-DBX-1, DSK-DBX-2).
dashboard-export-figure = Figure
dashboard-export-value = Value
dashboard-export-unit = Unit
dashboard-export-pick-presentation = Choose a PowerPoint presentation
dashboard-export-copy-name = { $presentation } with { $dashboard }

## Publish this folder automatically… in a folder menu (amended January 2026; user decision; SM-FP-1)

publish-folder = Publish this folder automatically…
publish-folder-title = Publish { $name } to Confluence automatically
publish-folder-body = Choose the Confluence page it goes under. You approve the link first. After that your own changes here are published automatically; Ordin's changes always wait for your approval.
publish-folder-sent = Publishing { $name } waits for your approval.
# The file dialog of minutes.import (OrdinDesktop.md DSK-MIN-2).
minutes-import-title = Choose a meeting transcript or recording
minutes-import-filter = Transcripts and recordings
