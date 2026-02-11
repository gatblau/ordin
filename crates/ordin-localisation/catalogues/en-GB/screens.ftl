# Words of the desktop shell: the side rail, the tree, the tabs, the conversation panel, the
# confirmation and quit dialogs and the error line (ScreensModule.md, Message catalogue entries).
# Other parts of the screens module keep their words in their own `screens-<area>.ftl` file; an id is
# defined in exactly one file. `{ $product }` and `{ $coordinator }` are filled from the brand.

rail-home = Home
rail-home-count = Home, { $count } new
rail-search = Search or go to…
rail-new-workspace = New workspace
rail-new-workspace-create = Create
# SM-TW-3: under New workspace, on this computer only (ADR-121).
rail-open-team-workspace = Open a team workspace…
rail-collapse-all = Collapse all
rail-expand = Expand
rail-workspaces = Workspaces
rail-resize = Resize the side panel
pane-rail-collapse = Hide the side panel
pane-rail-restore = Show the side panel
pane-conversation-collapse = Hide the conversation
pane-conversation-restore = Show the conversation
pane-agent-work = Make the conversation wide for working with agents
rail-settings = Settings
rail-server-on = On { $server }
rail-server-not-connected = Not connected · as of { $time }

tree-documents = Documents
tree-folder-empty = Nothing in here yet
# SM-DS-2: the Documents screen of a workspace with no place linked yet.
documents-no-places = No documents are linked to this workspace yet. Use Add to this workspace to link a folder, a site or a space.
tree-agents = Agents
tree-agent-made-of = What it's made of
tree-workflows = Workflows
tree-dashboards = Dashboards
tree-email = Email
tree-telegram = Telegram
tree-slack = Slack
tree-running = Running now
tree-knowledge = Knowledge
tree-node-menu = Actions for { $name }
tree-memory = Memory
# SM-TW-7, SM-TW-8: a team workspace's activity (ADR-121).
tree-team-activity = Team activity
# SM-TW-13 (amended January 2026, user decision): entries of a workspace's ⋯ menu; `tree-shared` is also
# the accessible name of a team workspace's icon on its row.
tree-share = Share with your team…
tree-shared = Shared with your team
# A document's or folder's ⋯ menu (amended January 2026, user decision): each asks Ordin in the conversation.
tree-add-to-knowledge = Add to knowledge
tree-add-to-memory = Add to memory
tree-ask-text-knowledge = Write a knowledge page from “{ $title }”.
tree-ask-text-memory = Add the facts in “{ $title }” to memory.

screen-settings = Settings
screen-diagnostics = Diagnostics
screen-approval = Approval
screen-document = Document
screen-run = Progress
run-screen-asked = Asked: { $brief }
run-screen-no-message = It left no message.
run-screen-try-again = Try again
run-screen-waiting = Nothing has started yet: the conversation needs you first.
screen-crew = Crew progress
screen-team-activity = Team activity
# SM-TF-1 to SM-TF-8: a task's form (ADR-119).
screen-task-form = Task
task-form-gone = This task isn't available any more.
task-form-close = Close
task-form-unreadable = This task can't be started from here. Ask Ordin in the conversation instead.
task-form-details-label = What should the agent do?
task-form-cancel = Cancel
task-form-optional = (optional)
task-form-required-mark = (required)
task-form-choose-document = Choose a document…
task-form-chosen-document = { $file }, in { $folder } of { $place }
task-form-chosen-document-top = { $file }, in { $place }
task-form-browser-title = Choose a document
task-form-browser-search = Search by file name
task-form-browser-none = No file has that name.
task-form-browser-empty = This folder is empty.
task-form-step-up = One more
task-form-step-down = One less
task-form-change-document = Choose another
task-form-add-document = Add
task-form-remove-document = Remove { $name }
task-form-documents-count = { $count } of { $max } documents
task-form-no-choice = None
task-form-access = When this task starts, { $agent } can read { $folders }, for this task and later ones.
task-form-required = Fill this in to start.
task-form-required-document = Choose a document to start.
task-form-required-documents = Choose at least two documents to start.
task-form-too-long = This is too long. Keep it to { $max } characters.
task-form-not-a-number = Enter a number, such as 2 or 3.5.
task-form-not-a-whole-number = Enter a whole number, such as 2.
task-form-not-a-date = Enter a whole date.
task-form-too-few-documents = This field takes two or more documents.
task-form-too-many-documents = Choose at most { $max } documents.
task-form-not-a-choice = Choose one of the options.
task-form-document-gone = { $name } isn't in this workspace any more. Choose another document.
task-form-changed = This task has changed since the form opened. Check the fields and try again.
task-form-starting = Starting…
task-form-started = Started. { $agent } is working on { $task }.
task-form-sent = Sent. The work has started.
screen-unknown = Page

# HS-TW-1, HS-TW-2: a colleague's added agent or access, waiting in Needs you.
needs-you-team-grant = { $person } added { $agent } — allow it?
needs-you-team-grant-unconfirmed = Someone added { $agent } — allow it? (says it was { $person })
needs-you-team-grant-gone = This was already answered.

conv-needs-you = Needs you
# A turn that ended with no reply (ScreensModule.md SM-RF-1).
conv-turn-failed = Ordin couldn't get an answer this time. Try again; if it happens again, choose Save logs for support… in Settings › Diagnostics.
conv-header-conversation = Conversation
conv-resize = Resize the conversation
conv-send = Send
# SM-CV-1: read before the person's own message in the conversation (screen readers).
conv-you-said = You said
conv-summarise = Summarise
conv-clear = Clear
conv-earlier = Earlier
conv-earlier-name = Earlier conversations
conv-earlier-count = Earlier ({ $count })
conv-earlier-label = Earlier conversations ({ $count })
# The working line below the last item while Ordin works on a turn (ScreensModule.md SM-WK-1).
ordin-working = Ordin is working…
ordin-working-for = Ordin is working… { $seconds } s
# $doing is one of the ordin-doing-* phrases (OrdinHost.md OH-WK-1).
ordin-working-doing = Ordin is { $doing }… { $seconds } s
# Shown when new items arrive while the person has scrolled up to read (SM-SCR-1).
conv-jump-newest = New messages ↓
conv-tab-close = Close tab · the conversation is kept

## Icon-only buttons (ScreensModule.md amendment *Icons*, SM-IC-3): accessible name and tooltip.
screen-tab-close = Close { $title }
screen-tab-menu = Tab actions for { $title }
screen-tab-menu-close = Close
screen-tab-menu-close-others = Close others
screen-tab-menu-close-right = Close tabs to the right
screen-tab-menu-close-all = Close all
conv-tab-close-named = Close the conversation tab for { $name }
conv-hide = Hide the conversation
message-dictate = Dictate
message-dictate-tip = Speak instead of typing
more-actions-for = More actions for { $name }
more-actions-tip = More actions

card-close = Close
card-open = Open
card-waiting = Waiting for you

home-new = new

confirm-cancel = Cancel
confirm-clear-title = Clear this conversation?
confirm-clear-body = Ordin forgets what was said here and starts a new conversation. This one stays readable in Earlier conversations. To carry decisions over, choose the key points first: Ordin lists them and you pick which ones go to What Ordin remembers. Summarise instead keeps this conversation going.

quit-title = Quit { $product }?
quit-body = { $count } agents are still working. They stop now; what they have written so far is kept.
quit-verb = Quit
quitting = Closing { $product }…
quitting-sessions = Closing { $product } — stopping agent sessions, please wait…

error-try-again = Try again
error-sign-in-again = Sign in again
error-reference = Reference { $reference }
error-needs-newer-version = This needs a newer version of ordin.

server-waiting-for = Waiting for { $server }
server-cannot-reach = Can't reach { $server } right now. ordin will keep trying while it's open.
server-ended = This computer is no longer connected to { $server }.
server-version-server-older = { $server } is older than ordin. Some things won't work until it's updated.
server-version-desktop-older = Update ordin on this computer to use { $server }.

# ---- Catch-up of words (ScreensModule.md, New messages) ------------------------------
screen-handover = Handover

# ---- Run-state labels (ScreensModule.md, rules from the HLD: run state table; `state-<stored state>`, underscores as hyphens) ----
# `paused` has three labels by condition in the table, so it has no plain id; its three ids follow (SM-RS-3).
state-queued = Waiting to start
state-running = Working
state-stopped-at-limit = Stopped — reached its limit
state-failed = Couldn't finish
state-interrupted = Interrupted
state-no-reply = No answer yet
state-abandoned = Not used (stopped crew)
state-completed = Done
state-cancelled = Stopped
state-paused-approval = Waiting for you
state-paused-slack = Waiting for you in Slack
state-paused-closed = Paused while ordin was closed
# The states only a crew run has (SM-RS-6).
state-starting = Starting
state-finished = Done
state-escalated = Handed back to you
state-killed = Stopped


## Add to this workspace and removing a link (ScreensModule.md amendment January 2026, SM-AW-1 to SM-AW-4, SM-RL-1, SM-RL-2).
add-title = Add to this workspace
add-intro = Nothing is copied. Ordin keeps a link to where things live and only reads them until you approve a change.
add-sharepoint = SharePoint
add-sharepoint-line = Company sites and libraries, typically Word and PowerPoint
add-confluence = Confluence
add-confluence-line = Spaces and page trees
add-local = This computer
add-local-line = A folder on this computer
add-code = A code repository on this computer
add-code-line = A folder that holds a Git repository; it appears under Code
add-email = Email…
add-email-line = Messages from people or companies, or a folder in your mailbox
add-linked = Linked
add-search-label = Find a site, library, space or page
add-search-none = Nothing found for “{ $query }”. Try other words.
add-add = Add
add-back = Back
add-sign-in-microsoft = Sign in with Microsoft
add-sign-in-confluence = Sign in to Confluence
add-sign-in-line = Sign in once with your work account so Ordin can reach it; your browser opens. ordin keeps the sign-in on this computer.
add-sign-in-reason = To list the places you can add to this workspace
add-confluence-not-configured = Confluence isn't set up on this computer yet.
add-open-settings = Open Settings
add-choose-folder = Choose a folder…
add-added = Added
add-added-line = { $name } is linked to this workspace.
add-reading = Reading { $read } of { $listed } documents…
add-show-documents = Show documents
add-more = Add something else
tree-add = Add to this workspace…
tree-add-documents = Add documents…
conv-delete = Delete…
conv-delete-title = Delete “{ $name }”?
conv-delete-body = The conversation and its messages are deleted from this computer. This can't be undone. Pages Ordin kept from it stay.
conv-delete-verb = Delete
conv-deleted = Deleted “{ $name }”.
tree-bin-done = Moved “{ $name }” to your computer's Bin.
tree-bin-undo = Undo
tree-remove-link = Remove from this workspace…
remove-link-title = Remove “{ $name }” from this workspace?
remove-link-body = Your documents stay where they are. Agents in this workspace can no longer use them.
remove-link-verb = Remove
remove-link-done = Removed the link to “{ $name }”. The documents are untouched where they live.
jira-remove-line = Nothing in Jira changes.
# Getting memory ready (ScreensModule.md amendment 2026-01-28, SM-MD-1 to SM-MD-6).
memory-models-title = Getting memory ready
memory-models-ask = To remember things for you, Ordin needs a one-time download of { $size } MB from { $host }.
memory-models-download = Download
memory-models-resume = Carry on downloading
memory-models-not-now = Not now
memory-models-progress = Downloading { $received } of { $size } MB…
memory-models-stop = Stop
memory-models-checking = Checking the download…
memory-models-ready = Memory is ready.
memory-models-try-again = Try again
# Renaming, archiving and the Bin for workspaces (ScreensModule.md, amendment January 2026, SM-WS-2 to SM-WS-8).
ws-rename = Rename…
ws-rename-field = New name for the workspace
ws-rename-verb = Rename
ws-renamed = Renamed the workspace to “{ $name }”.
ws-archive = Archive
ws-archived = Archived “{ $name }”. It's under Archived, below your workspaces.
ws-restore = Restore
ws-restored = Restored “{ $name }”.
ws-remove = Remove workspace…
ws-bin-title = Move this workspace to the Bin?
ws-bin-body = It goes to the Bin, with its links, conversation and history. You can restore it from there. Your documents stay where they are.
ws-bin-unsent = It also takes { $count } { $count ->
    [one] change
   *[other] changes
  } not yet sent back: { $documents }.
ws-bin-verb = Move to Bin
ws-binned = Moved “{ $name }” to the Bin. Your documents are where they were.
ws-undo = Undo
rail-archived = Archived
rail-archived-count = Archived, { $count }
rail-bin = Bin
rail-bin-count = Bin, { $count }
archived-title = Archived workspaces
archived-lead = Workspaces you archived. Restore one to work in it again.
archived-none = No workspace is archived.
bin-title = Bin
bin-lead = Workspaces you removed. Each keeps its links, conversation, history and changes not yet sent back until you empty the Bin. Your documents were never moved. A document you move to the Bin goes to your computer's own Bin or Trash, not to this one.
bin-removed-on = Removed { $date }
bin-restore = Restore
bin-delete = Delete now…
bin-empty = Empty Bin…
bin-none = The Bin is empty.
bin-restored = Restored “{ $name }”, with its conversation, history and changes not yet sent back.
bin-delete-title = Delete “{ $name }” now?
bin-delete-body = Its links, conversation and history are deleted. Your documents stay where they are. This can't be undone.
bin-delete-unsent = { $count } { $count ->
    [one] change
   *[other] changes
  } not yet sent back { $count ->
    [one] is
   *[other] are
  } lost: { $documents }.
bin-delete-verb = Delete
bin-deleted = Deleted “{ $name }”.
bin-empty-title = Empty the Bin?
bin-empty-body = { $count ->
    [one] One workspace is
   *[other] { $count } workspaces are
  } deleted, with their links, conversation and history: { $names }. Your documents stay where they are. This can't be undone.
bin-empty-verb = Empty Bin
bin-emptied = Emptied the Bin: { $count ->
    [one] one workspace
   *[other] { $count } workspaces
  } deleted.
