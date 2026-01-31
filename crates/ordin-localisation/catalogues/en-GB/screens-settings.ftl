# Words of Home, Search or go to…, Settings (every section), Diagnostics and Earlier conversations
# (ScreensModule.md: Message catalogue entries; the ADR-102, -106, -108, -109, -112/113, -114 and -115
# amendments; the January 2026 amendment's Actions group). Each id is defined here only; the shell's
# own words are in screens.ftl. `{ $coordinator }` is filled from the brand.

# ---- Home --------------------------------------------------------------------------------------------
home-waiting = Waiting for you
home-running = Running now
home-since = Since you last looked
home-all-workspaces = All workspaces
home-nothing-running = Nothing running now
home-show-progress = Show progress
home-open-handover-link = Open a handover link…
home-open-handover-link-hint = Paste the link from the Slack message or email.
home-handover-link-following = ordin now follows { $place }; the handover will appear when it's readable.

# ---- Search or go to… --------------------------------------------------------------------------------
search-go-to = Go to
search-found = Found in pages and documents
search-actions = Actions
search-footer = ↑↓ to move · Enter to open · Esc to close
search-other-workspace = To use this here, add it to this workspace.
search-action-home = Home
search-action-new-workspace = New workspace
search-action-add-documents = Add documents
search-action-summarise = Summarise
search-action-clear = Clear
search-action-earlier-conversations = Earlier conversations
search-action-settings = Settings

# ---- Settings: section names -------------------------------------------------------------------------
settings-section-sign-ins = Sign-ins
settings-section-agent-repositories = Agent repositories
settings-section-email = Email
settings-section-telegram = Telegram
settings-section-slack = Slack
settings-section-jira = Jira
settings-section-servers = Servers
settings-section-dictation = Dictation
settings-section-how-ordin-talks = How { $coordinator } talks to you
settings-section-language = Language
settings-section-using-your-plan = Using your plan
settings-section-notifications = Notifications
settings-section-history = History
settings-section-display = Display
settings-section-code = Code
settings-section-what-ordin-remembers = What { $coordinator } remembers
settings-section-knowledge-agents = Knowledge agents
settings-section-diagnostics = Diagnostics
settings-section-about = About ordin

# ---- Settings: History and Display -------------------------------------------------------------------
settings-history-line = Keep conversations and history for { $days } days
settings-history-invalid = Enter a whole number of days, 1 or more.
settings-history-only-way = This is the only way earlier conversations are removed.
settings-appearance = Appearance: Same as this computer / Light / Dark

# ---- Settings: Sign-ins ------------------------------------------------------------------------------
signin-gitlab-paste-token = Paste a GitLab access token
signin-import-certificate = Import a certificate…

# ---- Settings: Slack ---------------------------------------------------------------------------------
settings-slack-fixed = Every post is shown to you before it is posted, unless you set a rule for it.
settings-slack-signed-in = Signed in to { $workspace } as { $name }.
settings-slack-sign-in = Sign in to Slack…
settings-slack-own-app = Connect your own Slack app…
settings-slack-own-app-steps = Create an app in your Slack workspace from this description, then paste its client ID here.
settings-slack-not-approved = Your Slack workspace needs an administrator to approve apps, so ordin can't connect. Handovers still go by email.
settings-slack-rules = Posted without asking
settings-slack-rule-line = { $kind } in #{ $channel } · up to { $cap } a day · { $state }
settings-slack-rule-off = Turn off
settings-slack-rules-off-by-company = Your company asks that every Slack post is shown to you first.
settings-slack-more-permissions = To send someone a Slack message and find them by their email address, ordin needs two more Slack permissions.

# ---- Settings: Jira ----------------------------------------------------------------------------------
settings-jira-intro = Connect Jira so Ordin can read the issues you link and hand work over on them. ordin writes to Jira only when you approve a handover.
settings-jira-site = Your Jira site, for example acme.atlassian.net
# SM-JS-2: the site field is empty, so nothing was sent.
settings-jira-site-needed = Type your Jira site, for example acme.atlassian.net.
settings-jira-email = The email address you sign in to Jira with
settings-jira-token = API token
settings-jira-token-guide = In your Atlassian account, open Security, then API tokens, create a token and paste it here. You don't need anyone's permission.
settings-jira-connect = Connect
settings-jira-oauth = Sign in with Atlassian…
settings-jira-oauth-unavailable = Your company hasn't set up Atlassian sign-in. Use an API token instead.
settings-jira-connected = Connected to { $site } as { $name }
settings-jira-disconnect = Disconnect Jira…
settings-jira-other-site = ordin is already connected to another Jira site. Disconnect it first.

# ---- Settings: Servers -------------------------------------------------------------------------------
server-connect = Connect to a server…
server-address = Its address
server-code = The code from whoever runs your ordin server
server-connect-verb = Connect
server-code-failed = That code didn't work. Ask for a new one from whoever runs your ordin server.
server-disconnect = Disconnect this computer…
server-disconnect-title = Disconnect this computer from { $server }?
server-disconnect-body = The server keeps working. You can connect again with a new code.
server-remove = Remove this server
server-connect-again = Connect again…
server-languages = Languages…
server-update-available = { $server } can update to ordin { $version }.
server-update-button = Update server…
server-update-confirm-title = Update { $server } to ordin { $version }?
server-update-confirm-body = Work running on { $server } stops and is marked as interrupted. The server restarts within a minute and then continues.
server-update-confirm-verb = Update server
server-update-started = { $server } is updating. It will be back within a minute.
server-update-notes = What's new in this version

# ---- Settings: About and updates (ADR-114, ADR-072) --------------------------------------------------
about-version = Version { $version }
about-check = Check for updates
about-checking = Checking for updates…
about-up-to-date = ordin is up to date.
about-downloading = Getting ordin { $version } ready…
about-ready = ordin { $version } is ready.
about-could-not-check = ordin couldn't check for updates. Try again later.
about-whats-new = What's new
about-whats-new-none = There are no notes for this version.
update-ready-line = ordin { $version } is ready.
update-restart = Restart to update
update-later = Not now
update-restarting = Restarting to update…
update-ask-administrator = ordin { $version } is available — ask your administrator to update.
whats-new-title = What's new in ordin { $version }
whats-new-got-it = Got it

# ---- Settings: Knowledge agents (ADR-112, ADR-115) ---------------------------------------------------
knowledge-agents-intro = A knowledge agent answers from your organisation's knowledge. When ordin's own memory isn't enough, it asks the agents you connect here. Only your question is sent, never your conversation or files.
knowledge-agents-workspace = For workspace
knowledge-agents-none = This workspace isn't connected to a knowledge agent. It works as it always has.
knowledge-agents-connect = Connect a knowledge agent…
knowledge-agents-connect-title = Connect a knowledge agent
knowledge-agents-address = The knowledge agent's address
knowledge-agents-address-help = Ask the team that runs it for its address. You sign in with your company account.
knowledge-agents-connect-verb = Connect
knowledge-agents-record-on = This knowledge agent keeps a record of which sources each answer used, for { $days } days. The record never holds your question or your name.
knowledge-agents-record-off = This knowledge agent keeps no record of which sources its answers used.
knowledge-agents-connected = Connected. Ordin will ask { $name } when its own memory isn't enough.
knowledge-agents-set-by-company = Set by your company
knowledge-agents-last-answered = Last answered { $when }
knowledge-agents-never-answered = Not asked yet
knowledge-agents-details = Details
knowledge-agents-places = Reads { $count } places
knowledge-agents-as-of = Up to date as of { $when }
knowledge-agents-remove = Remove
knowledge-agents-remove-confirm = Stop asking { $name }? Ordin will answer from this workspace's own memory only.
knowledge-agents-removed = Removed. Ordin won't ask { $name } again.
knowledge-agents-sign-in-to-include = Sign in to { $system } in ordin to include what you can read there.
knowledge-agents-account-linked = { $system }: your account is linked.
knowledge-agents-account-refused = { $system } didn't accept ordin's sign-in for this knowledge agent.

# ---- Settings: Language (ADR-109) --------------------------------------------------------------------
language-picker-label = Language ordin uses
language-follow-computer = Follow this computer
language-made-by-you = Made by you
language-imported = Imported
language-edit = Edit wording
language-export = Export…
language-import = Import…
language-make = Make a new language…
language-yours = Your languages
language-state-drafting = Drafting { $done } of { $total }
language-stop-drafting = Stop drafting
language-state-needs-checking = Needs checking: { $count } approval { $count ->
    [one] message
   *[other] messages
}
language-state-ready-to-use = Checked — ready to use
language-state-needs-changing = { $count } { $count ->
    [one] message needs
   *[other] messages need
} changing after the update
language-state-in-english = { $count } { $count ->
    [one] message
   *[other] messages
} still in English
language-ask-ordin-draft = Ask Ordin to draft them
language-state-in-use = In use
language-open = Open
language-screen-title = Wording: { $language }
language-use = Use this language
language-use-check-first = Check the approval wording first
language-restore-all = Restore all…
language-restore-all-verb = Restore all
confirm-restore-all-title = Restore all of { $language } to the wording ordin came with?
confirm-restore-all-body = Your { $count } changes are removed.
language-remove = Remove this language…
language-remove-verb = Remove
confirm-remove-language-title = Remove { $language }?
confirm-remove-language-body = Its wording is deleted from this computer. Export it first if you may want it again.
language-search-label = Find wording
language-area-label = Part of ordin
language-area-all = All parts
language-only-needs-checking = Needs checking
language-only-in-english = In English
language-only-edited = Edited
language-only-needs-changing = Needs changing
language-results = { $count } { $count ->
    [one] message
   *[other] messages
}
language-shows-english = Shows in English
language-source-shipped = Came with ordin
language-source-edited = Edited by you
language-source-drafted = Drafted by Ordin
language-source-imported = Imported
language-source-english = In English
language-approval-wording = Approval wording
language-edit-message = Edit
language-restore = Restore
language-edit-label = Wording in { $language }
language-mark-checked = Mark as checked
language-mark-all-checked = Mark all shown as checked
language-make-name = Language name
language-make-tag = Language code
language-make-tag-hint = For example ja for Japanese, or pt-BR for Portuguese as spoken in Brazil
language-make-estimate = Estimated use: { $band }, about { $min } to { $max } model turns, for { $count } messages.
language-make-no-estimate = { $count } messages to draft.
language-band-light = light
language-band-medium = medium
language-band-heavy = heavy
language-make-review-line = Before you use it, you check the wording on approval cards.
confirm-replace-language-title = Replace your { $language }?
confirm-replace-language-body = The wording in the file replaces the wording you have now.
language-replace-verb = Replace
language-import-report = Imported { $stored } messages; { $unknown } not known here were left out; { $english } show in English.
language-import-left-out = { $count } { $count ->
    [one] message didn't pass the checks and was
   *[other] messages didn't pass the checks and were
} left out.
language-show-which = Show which

# ---- Earlier conversations (ADR-108) -----------------------------------------------------------------
conv-picker-label = Conversation: { $name }
conv-picker-new = New conversation
conv-picker-current = Current
conv-picker-all = All earlier conversations ({ $count })
conv-picker-none = No earlier conversations
conv-picker-date = { $date } · { $count } messages
conv-continue = Continue this conversation
conv-read-it = Read it
conv-continue-waits = Waits until the crew finishes
confirm-continue-title = Continue this conversation?
confirm-continue-body = Ordin stops working from the current conversation and works from “{ $name }” instead. Both are kept, and nothing is deleted.
confirm-continue-verb = Continue
confirm-clear-keep-points = Keep the key points first
conv-continued-line = Continued { $time } · earlier messages ({ $count })
conv-continued-show = Show
conv-continued-hide = Hide
conv-continued-announce = Continuing “{ $name }”
conv-continued-from = Continued { $time }
conv-item-moved = Moved to the current conversation

# ---- Run again (Home) --------------------------------------------------------------------------------
progress-run-again = Run again…

# ---- Words the spec quotes as button or sentence text without giving them an id ----------------------
# SPEC-GAP-7: ScreensModule.md names these as **Save**, **Next**, **Keep it**, **Sign out**, **Allow…** and
# the https sentence of UI_KNOWLEDGE_ADDRESS_NOT_HTTPS but lists no message id. The ids below are proposals
# to be adopted by the spec; the text is the spec's own.
knowledge-agents-address-not-https = Use an address that starts with https://.
knowledge-agents-keep-it = Keep it
language-save = Save
language-make-next = Next
settings-slack-sign-out = Sign out
settings-slack-allow = Allow…
settings-show-more = Show more
# **Ask Ordin about it** on an earlier conversation (step 9, Earlier conversations row) is quoted without an id.
conv-ask-about = Ask { $coordinator } about it


# ---- Catch-up of words (ScreensModule.md, New messages) ------------------------------
home-nothing-waiting = Nothing is waiting for you
home-nothing-since = Nothing new since you last looked
home-group-hosted-agents = Hosted agents
home-group-no-workspace = Not in a workspace
search-result-count = { $count ->
    [one] { $count } result
   *[other] { $count } results
}
search-no-results = No results
server-paste-line-hint = Or paste the one line you were given here and leave the code empty.
server-update-self-unavailable = This server can't update itself. Whoever runs it installs new versions.
settings-slack-rule-kind-handover-notices = Handover notices
settings-slack-rule-kind-ordin-answers = Ordin's answers
settings-slack-rule-state-on = on
settings-slack-rule-state-off = off
settings-set-by-company = Set by your company
settings-repos-locked = Can't be removed
settings-repos-format-ordin = ordin format
settings-repos-format-apm = APM format
settings-repos-agent-count = { $count ->
    [one] 1 agent
   *[other] { $count } agents
}
settings-repos-show-parts = Show its parts
settings-repos-rename = Rename…
settings-repos-rename-field = New name for this agent repository
settings-repos-rename-verb = Rename
settings-repos-remove = Remove…
settings-repos-remove-title = Remove { $name }?
settings-repos-remove-body = ordin stops reading this agent repository. You can add it again later.
settings-repos-remove-verb = Remove
settings-repos-add = Add an agent repository…
settings-repos-add-address = The agent repository's web address
settings-repos-add-look = Look it up
settings-repos-add-found = { $count ->
    [one] 1 agent found · { $format }
   *[other] { $count } agents found · { $format }
}
settings-repos-add-name = Name shown in ordin
settings-repos-add-cannot-publish = You can add agents from it, but you can't publish to it.
settings-repos-add-verb = Add
settings-email-fixed = Every email is shown to you before it is sent. This can't be turned off.
settings-email-mailbox = Your mailbox
settings-email-signed-in-password = Signed in with your password, kept in this computer's keychain
settings-email-signed-in-account = Signed in with your company account
settings-email-change-password = Change password
settings-email-sign-out = Sign out of email
settings-email-check-refused = Your mail server didn't accept this password. Check it and try again.
settings-email-check-not-secure = Your company's mail server can't be reached securely. Ask IT.
settings-email-add-signature = Add my signature
settings-email-signature = Your signature
settings-email-foot-line = Say at the bottom that an agent drafted it
settings-email-foot-line-required = Required by your company
settings-email-check = Check for new email
settings-email-check-every-15 = Every 15 minutes
settings-email-check-on-open = Only when I open the workspace
settings-email-workspaces = Email in your workspaces
settings-telegram-fixed = Every post is shown to you before it is posted. This can't be turned off.
settings-telegram-not-allowed = Your company doesn't allow Telegram here.
settings-telegram-tools-off = Telegram isn't available here.
settings-telegram-no-app = Telegram isn't on this computer. Install it to link Telegram groups here.
settings-telegram-no-bot = No Telegram bot is set up for ordin.
settings-telegram-bot = Your bot: { $name } (@{ $username })
settings-telegram-admin-note = Your bot sees only messages addressed to it in groups where it isn't an administrator.
settings-telegram-remove = Remove Telegram
settings-telegram-set-up = Set up Telegram
settings-telegram-key = Paste the key BotFather sent you
settings-telegram-connect = Connect
settings-dictation-available = Dictation is ready in { $language }.
settings-dictation-needs-assets = Dictation gets ready the first time you use it.
settings-voice-fixed = Ordin always uses plain words and keeps it short. Choose how it sounds.
settings-voice-tone = Tone
settings-voice-tone-friendly = Friendly
settings-voice-tone-neutral = Neutral
settings-voice-tone-warm = Warm
settings-voice-tone-direct = Direct
settings-voice-formality = Formality
settings-voice-formality-everyday = Everyday
settings-voice-formality-formal = Formal
settings-voice-length = Length
settings-voice-length-brief = Brief
settings-voice-length-shortest = Shortest
settings-voice-call-me = Call me
settings-voice-notes = Notes for Ordin
settings-voice-notes-example = I'm new to procurement; explain a term the first time you use it
settings-voice-notes-limit = Notes change how Ordin explains, never what it may do
settings-voice-company-default = Your company's default
settings-plan-today = Today
settings-plan-today-line = Today you've used about { $percent } of a typical day's plan.
settings-plan-details = Details
settings-plan-today-details = { $turns } model turns · { $percent } on smaller models · { $answered } answered by ordin without the model
settings-plan-choice = How Ordin uses your plan
settings-plan-choice-save = Save my plan
settings-plan-choice-balanced = Balanced
settings-plan-choice-best = Best results
settings-plan-choice-save-line = Smaller models wherever they do the job; asks before medium or heavy work
settings-plan-choice-balanced-line = Ordin picks the model for each step; the most capable only where it matters
settings-plan-choice-best-line = The most capable models for talking, planning and writing
settings-plan-choice-custom = Custom
settings-plan-without-model = Answer simple requests without the model
settings-plan-reuse = Reuse an answer when nothing has changed
settings-plan-only-needed = Give agents only the parts they need
settings-plan-continue-after-break = After a break of more than an hour, continue from a summary
settings-plan-summarise = Summarise long conversations
settings-plan-summarise-early = Early
settings-plan-summarise-nearly-full = When the conversation is nearly full
settings-plan-summarise-only-asked = Only when I ask
settings-plan-ask-before = Ask before work estimated as
settings-plan-ask-heavy = Heavy
settings-plan-ask-medium-or-heavy = Medium or heavy
settings-plan-ask-never = Never ask
settings-plan-agents-at-once = Agents working at the same time
settings-plan-more = More choices
settings-plan-show-usage = Show usage on each run
settings-plan-thinking = How much Ordin thinks first
settings-plan-thinking-low = Less
settings-plan-thinking-automatic = Ordin decides
settings-plan-thinking-high = More
settings-notify-intro = ordin tells you outside its window only about these.
settings-notify-approvals = Approvals waiting for you
settings-notify-finished-work = Finished tasks and crews
settings-notify-questions = Questions that stop work until you answer
settings-notify-draft-reviews = Reviews of your drafts on GitHub
settings-notify-handovers = Handovers to you and returned to you
settings-code-open-in = Open code in
settings-code-no-ide = No IDE was found on this computer.
settings-code-languages = Code checking by language
settings-code-pinned = version { $version }
settings-code-from-company = From your company
settings-code-from-agent = From an agent
settings-code-on = On
settings-code-restart = Restart
settings-memory-workspace = In the workspace { $workspace }
settings-memory-no-workspace = Open a workspace to see what Ordin remembers in it.
signin-kind-github = GitHub
signin-kind-gitlab = GitLab
signin-kind-slack = Slack
signin-kind-confluence = Confluence
signin-kind-microsoft = Microsoft
signin-kind-google = Google
signin-kind-jira = Jira
signin-kind-oidc = Your company account
signin-kind-mail-server-single-sign-on = Company email
signin-kind-mail-server-password = Company email
signin-kind-mail-server-app-password = Company email
signin-state-signed-in = Signed in
signin-state-needs-sign-in = Sign in again to keep using it
signin-state-signed-out = Asked for the first time Ordin needs it
signin-expires = until { $date }
signin-sign-in = Sign in
signin-sign-out = Sign out
signin-browser-opened = Finish signing in in your browser.
signin-device-code = Open { $address } and enter this code:
signin-copy-code = Copy the code
signin-mail-title = Sign in to your company's email
signin-mail-password-note = Your company's mail server doesn't use a browser sign-in, so this is the one password ordin asks for. It goes only to your company's mail server and is kept in this computer's keychain.
signin-mail-address = Email address
signin-mail-address-hint = Filled in from your company account. Change it only if you use another address for email.
signin-mail-app-password = App password
signin-mail-password = Password
signin-show = Show
signin-mail-keychain-note = ordin keeps this in this computer's keychain and uses it only to reach your company's mail server.
signin-certificate-imported = Certificate { $name } imported. It's valid until { $date }.
diagnostics-journal-title = Journal events
diagnostics-window-title = This window
diagnostics-support-export = Save logs for support…
diagnostics-support-pick = Choose up to 20 runs to include.
diagnostics-support-limit = You can include up to 20 runs.
diagnostics-support-note = The file holds the chosen runs' logs, their journal entries and the versions in use. It holds no tokens, message text or file content.
diagnostics-support-save = Save…
diagnostics-support-saved = Saved to { $path }
