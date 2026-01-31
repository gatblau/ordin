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
settings-section-what-ordin-remembers = Memory
settings-section-knowledge-agents = Knowledge agents
settings-section-diagnostics = Diagnostics
settings-section-about = Updates

# ---- Settings: History and Display -------------------------------------------------------------------
settings-history-line = Keep conversations and history for { $days } days
settings-history-invalid = Enter a whole number of days, 1 or more.
settings-history-only-way = This is the only way earlier conversations are removed. An approval still waiting and work still running are never removed.
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
settings-jira-intro = Connect Jira so ordin can read the issues you link and hand work over on them. ordin writes to Jira only when you approve a handover.
settings-jira-site = Your Jira site, for example acme.atlassian.net
# SM-JS-2: the site field is empty, so nothing was sent.
settings-jira-site-needed = Type your Jira site, for example acme.atlassian.net.
settings-jira-email = The email address you sign in to Jira with
settings-jira-token = API token
settings-jira-token-guide = In your Atlassian account, open Security, then API tokens, create a token and paste it here. You don't need anyone's permission.
settings-jira-connect = Connect
settings-jira-oauth = Sign in with Atlassian…
settings-jira-oauth-unavailable = Signing in with Atlassian isn't available in this version yet. Use an API token instead.
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
server-remove = Remove this server…
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
update-ask-administrator = ordin { $version } is available. This copy was installed for everyone on this computer, so install the new version the same way.
whats-new-title = What's new in ordin { $version }
whats-new-got-it = Got it

# ---- Settings: Knowledge agents (ADR-112, ADR-115) ---------------------------------------------------
knowledge-agents-intro = A knowledge agent answers from your organisation's knowledge. When Ordin's own memory isn't enough, it asks the agents you connect here. Only your question is sent, never your conversation or files.
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
language-follow-computer = Same as this computer
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
settings-email-check-not-secure = Your company's mail server can't be reached securely, so ordin didn't connect. Try again later.
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
settings-telegram-key = The bot's key
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
settings-plan-today-line = Today you've used { $share } of a typical day's plan.
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
settings-plan-summarise-early = Early (saves most)
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
settings-notify-intro = ordin shows a notification on your computer only for what's switched on here.
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

# ---- Settings walk-through (ScreensModule.md, Settings walk-through amendment; amended January 2026 (user decision)) ----
settings-section-menu = Section
settings-default = (default)
settings-saved = Saved
signin-intro = Ordin uses these to read and publish your documents, code and messages. Your passwords stay in this computer's keychain.
signin-service = { $name }, { $for }
signin-for-github = for your agent library and GitHub documents
signin-for-gitlab = for GitLab repositories and documents
signin-for-microsoft = for SharePoint and email
signin-for-google = for email
signin-for-confluence = for your company's pages
signin-for-slack = for handovers and messages in Slack
signin-for-mail = for your company's email
signin-for-oidc = for knowledge agents and your company's servers
signin-signed-in-until = Signed in until { $date }
signin-signed-out-of = Signed out of { $service }.
signin-gitlab-token-hint = Make one in GitLab under Preferences, then Personal access tokens. It's kept in this computer's keychain.
signin-gitlab-token-empty = Paste a token first.
signin-gitlab-save = Save token
# SM-NS-2.6 (amended January 2026; user decision; Approved by the user January 2026): GitHub with no GitHub App in this build.
signin-github-paste-token = Paste a GitHub personal access token
signin-github-token-why = This copy of ordin connects to GitHub with a token you make yourself. You don't need anyone's permission.
signin-github-token-step-1 = On GitHub, open Settings, then Developer settings, then Personal access tokens, and choose Fine-grained tokens.
signin-github-token-step-2 = Generate a new token. Under Repository access choose the repositories ordin may use: your agent library and the repositories that hold your documents.
signin-github-token-step-3 = Under Repository permissions set Contents to Read and write and Pull requests to Read and write, so ordin can read and publish your documents and agents and follow reviews of your drafts. Then generate the token and paste it here.
signin-github-open-token-page = Open GitHub's token page
signin-github-token-hint = ordin checks the token with GitHub before keeping it in this computer's keychain.
signin-github-token-empty = Paste a token first.
signin-github-token-refused = GitHub didn't accept that token. Check you copied all of it and that it hasn't expired, or open GitHub's token page and make a new one.
signin-github-save = Save token
signin-not-available = Not available in this version yet
signin-row-show = Show the ways to sign in to { $service }
signin-row-hide = Hide the ways to sign in to { $service }
signin-way-one-click = With one click
signin-way-token = With a token
signin-way-ssh = With an SSH key
signin-github-ssh-why = An SSH key lets ordin fetch and send your work on GitHub without a token. Some features, such as pull requests, still need the token.
signin-github-ssh-choose = SSH key
signin-github-ssh-none = Don't use an SSH key
signin-github-ssh-no-keys = You have no SSH keys yet. Make one here.
signin-github-ssh-create = Make a new key
signin-github-ssh-public = Your public key
signin-github-ssh-add = Add this public key to your GitHub settings under SSH keys, then check it works.
signin-github-ssh-open-page = Open GitHub's SSH key page
signin-github-ssh-copy = Copy the public key
signin-github-ssh-copied = Public key copied.
signin-github-ssh-check = Check it works
signin-github-ssh-checking = Checking with GitHub…
signin-github-ssh-works = It works. ordin uses this key for your GitHub repositories.
signin-github-ssh-refused = GitHub didn't accept this key. Add the public key in your GitHub settings and try again. A key protected by a passphrase can't be used.
signin-github-ssh-unreachable = ordin couldn't reach GitHub. Check your internet connection and try again.
signin-hide = Hide
signin-mail-password-empty = Type your password first.
signin-mail-sso-note = Your company's mail server uses your company account. Type your email address, then finish signing in in your browser.
# SM-NS-2.11 (amended January 2026; user decision; Approved by the user January 2026): Confluence with an
# API token the person makes; confluence.connect_token and confluence.connect_with_jira.
signin-way-api-token = With an API token
signin-confluence-token-why = Connect Confluence with an API token you make yourself in your Atlassian account. You don't need anyone's permission.
signin-confluence-site = Your Confluence site
signin-confluence-site-placeholder = https://your-company.atlassian.net
signin-confluence-site-needed = Type your Confluence site, for example https://your-company.atlassian.net.
signin-confluence-email = The email address you sign in to Atlassian with
signin-confluence-token-how = On Atlassian's token page choose Create API token, give it a name such as ordin, then copy the token and paste it below.
signin-confluence-open-token-page = Open Atlassian's token page
signin-confluence-token = API token
signin-confluence-token-hint = ordin checks the token with Confluence before keeping it in this computer's keychain.
signin-confluence-token-empty = Paste a token first.
signin-confluence-connect = Connect
signin-confluence-use-jira = Use my Jira sign-in for { $site }
signin-confluence-connected = Connected to { $site } as { $name }
signin-confluence-disconnect = Disconnect
signin-confluence-disconnected = Disconnected Confluence and deleted the token from this computer's keychain.
settings-email-not-signed-in = Not signed in — asked the first time a workspace links email
settings-email-signed-out = Signed out of email, and your password is removed from this computer's keychain. Workspaces keep their email links; nothing is read until you sign in again.
settings-email-signed-out-account = Signed out of email. Workspaces keep their email links; nothing is read until you sign in again.
settings-email-workspaces-none = No workspace links email yet. Add it from a workspace's "Add to this workspace".
settings-telegram-set-up-line = ordin posts through a Telegram bot of your own, so posts come from the bot, never from you. Three steps, once on this computer.
settings-telegram-step-make = Make your bot
settings-telegram-step-make-line = In Telegram, open a chat with @BotFather, Telegram's own helper for making bots. Send /newbot, then a name. BotFather answers with a key.
# SM-SW-5 (amended January 2026 (user decision)): app.open_web_page { page: telegram_botfather }.
settings-telegram-open-botfather = Open BotFather in Telegram
settings-telegram-step-paste = Paste the key BotFather sent you
settings-telegram-key-hint = ordin keeps it in this computer's keychain. Never paste it into a chat.
settings-telegram-key-empty = Paste the key first.
settings-telegram-step-group = Add your bot to a group
settings-telegram-step-group-line = Add your bot to a Telegram group as an administrator, so it can read the group. Then link the group from a workspace's "Add to this workspace".
settings-telegram-removed = Removed Telegram from ordin on this computer, and deleted the bot's key from the keychain. Workspaces keep their Telegram links; nothing is read until you connect again.
settings-plan-today-over = Today you've used more than a typical day's plan.
settings-plan-share-small = a small part
settings-plan-share-tenth = about a tenth
settings-plan-share-fifth = about a fifth
settings-plan-share-third = about a third
settings-plan-share-half = about half
settings-plan-share-most = most
settings-plan-choice-custom-line = You chose the model for each kind of work under More choices.
settings-plan-without-model-line = Opening, finding, going to and "what's new" are done by ordin itself and use none of your plan.
settings-plan-reuse-line = The same question over the same versions of the same documents, within 7 days, shows the earlier answer, with Ask again. Never an action or a change.
settings-plan-only-needed-line = Long documents and long results are cut to the parts that matter; an agent can still read the rest.
settings-plan-continue-after-break-line = After an hour the next message would reread the whole conversation; a summary costs less.
settings-plan-summarise-line = Summarising is one large message itself, so early pays off only in a conversation that goes on.
settings-plan-ask-before-line = Crews and tasks over many documents show their estimated use first.
settings-plan-agents-at-once-line = More at once finishes sooner and uses your plan faster.
settings-plan-agents-fewer = Fewer agents at the same time
settings-plan-agents-more = More agents at the same time
settings-plan-show-usage-line = How much of your plan each run used, on its progress.
settings-plan-thinking-line = More thinking gives better answers to hard questions and uses more of your plan.
# Settings › Using your plan, its cards (amended January 2026; user decision; Approved by the user January 2026).
settings-plan-group-less = Ways to use less
settings-plan-group-limits = Limits
settings-code-open-in-line = IDEs found on this computer. Each one ticked appears as "Open in …" on the Code group, its repository and each file.
settings-code-languages-line = They give the code editor and agents completion, errors and go to definition. Every language is highlighted in the editor with or without one.
settings-code-no-languages = No language has code checking yet. Every language is still highlighted in the editor.
settings-code-on-for = Code checking for { $language }
settings-code-server-for = Language server for { $language }
settings-code-restarted = Restarted the language server for { $language }.
open-in-ide = Open in { $ide }
open-in-ide-menu = Open in an IDE
open-in-ide-opened = Opened { $name } in { $ide }.
settings-history-lower-title = Remove conversations older than { $days } days?
settings-history-lower-body = They are removed from this computer at the next clean-up, within a day. An approval still waiting and work still running are never removed.
settings-history-lower-verb = Remove older conversations
settings-history-company-max = Your company keeps history for at most { $days } days.
settings-history-saved = Saved. Conversations and history are kept for { $days } days.
settings-repos-intro = Where the agents in your agent library come from.
settings-repos-none = No agent repositories yet. Add one to bring its agents into your library.
settings-repos-on = On { $host }
settings-repos-show-parts-needs-workspace = Open a workspace to see the parts of an agent repository you publish to.
settings-repos-rename-title = Rename { $name }
settings-repos-add-address-hint = For example https://github.com/northwind/finance-agents
settings-repos-added = Added { $name }. { $count ->
    [one] Its agent is
   *[other] Its { $count } agents are
} in your agent library.
settings-slack-tools-off = Slack isn't available here.
settings-slack-no-app = Your company hasn't set up a Slack app for ordin.
settings-slack-sign-in-not-here = Signing in to Slack isn't offered on this computer.
settings-slack-client-id = Client ID (on your app's Basic Information page)
settings-slack-description = App description to copy
settings-slack-copy = Copy
settings-slack-copied = Copied.
settings-slack-connect = Connect
settings-jira-connected-site = Connected to { $site }
settings-jira-disconnect-title = Disconnect Jira?
settings-jira-disconnect-body = The token is deleted from this computer's keychain. Linked issues stay in their workspaces; nothing is read until you connect again.
settings-jira-disconnect-verb = Disconnect
settings-jira-disconnected = Disconnected Jira on this computer and deleted the token from the keychain. Linked issues stay in their workspaces; nothing is read until you connect again.
settings-jira-token-step-1 = Choose Open Atlassian's token page below and sign in to your Atlassian account. If the page doesn't open, choose Copy the address and paste it into your browser.
settings-jira-token-step-2 = Choose Create API token, give it a name such as ordin, and copy the token.
settings-jira-token-step-3 = Paste the token below. You don't need anyone's permission.
# SM-SW-11 (amended January 2026 (user decision)): app.open_web_page { page: atlassian_api_tokens }.
settings-jira-open-token-page = Open Atlassian's token page
settings-jira-copy-token-page = Copy the address
settings-jira-token-page-copied = Copied.
settings-jira-token-note = The token is kept in this computer's keychain. ordin uses it only to read the issues you link and to hand work over after you approve.
server-none = No servers yet. Connect to one with the code from whoever runs it.
server-state-connected = Connected
server-state-not-connected = Not connected
server-address-hint = For example ordin.northwind-group.com.
server-connect-again-title = Connect again to { $server }
server-remove-title = Remove { $server } from this computer?
server-remove-body = This computer disconnects and forgets { $server }, with the key it used to connect. The server keeps working. To connect again you need a new code from whoever runs it.
server-remove-verb = Remove
server-removed = Removed { $server } from this computer.
language-yours-none = You haven't made or imported a language yet.
language-export-pick = Choose a language to save its wording as a file.
knowledge-agents-no-workspace = Open or make a workspace to connect a knowledge agent to it.
settings-voice-call-me-hint = Used in greetings only. Leave empty for no name.
settings-voice-notes-count = { $count } of { $max }
settings-dictation-where = Start dictation with the microphone button in the message box or a form field. Your speech stays on this computer and is never kept; you check the words before you send them.
about-title = Updates
# SM-NS-1.9 (amended January 2026; user decision; Approved by the user January 2026): Settings › Memory holds settings only.
settings-memory-intro = Choose how Ordin keeps, shows and uses what it remembers. To see or change the facts themselves, open What Ordin remembers in the side list.
settings-memory-group-keeps = What Ordin keeps
settings-memory-keep = When Ordin learns something in a conversation
settings-memory-keep-on-its-own = Keep it on its own
settings-memory-keep-ask-first = Ask me first
settings-memory-keep-hint = With Ask me first, new facts wait under To check until you keep them.
settings-memory-sources = Learn from
settings-memory-source-conversations = Conversations
settings-memory-source-conversations-hint = What you, Ordin and its agents say while working together.
settings-memory-source-documents = Documents
settings-memory-source-documents-hint = Files and pages Ordin and its agents read for you.
settings-memory-removal = Suggest removing facts not used for
settings-memory-removal-faded = As long as Ordin thinks they're useful
settings-memory-removal-days = { $days } days
settings-memory-removal-hint = Ordin only suggests. Nothing is removed unless you choose Forget.
settings-memory-group-shown = How memory is shown
settings-memory-group-by = Group facts by
settings-memory-group-by-hint = Used in each workspace until you choose another grouping there.
settings-memory-page-size = Facts shown before Show more
settings-memory-page-size-hint = How many facts each group shows at first.
settings-memory-details-open = Show each fact's details straight away
settings-memory-details-open-hint = When off, details stay closed until you open them.
settings-memory-group-uses = How Ordin uses memory
settings-memory-use-everywhere = Use facts in other workspaces
settings-memory-use-everywhere-hint = Facts you chose to remember everywhere are used in every workspace. When off, each workspace uses only its own facts.
settings-memory-say-which = Say which facts an answer used
settings-memory-say-which-hint = Ordin ends an answer by naming the remembered facts it relied on. Starts with the next conversation.
signin-kind-github = GitHub
signin-kind-gitlab = GitLab
signin-kind-slack = Slack
signin-kind-confluence = Confluence
signin-kind-microsoft = Microsoft 365
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
diagnostics-support-note = The file holds your newest 20 runs' records and the last 3 days of ordin's logs. It holds no document or conversation text.
diagnostics-support-nothing = There is nothing to save yet.
diagnostics-support-saved = Saved to { $path }
diagnostics-clear = Clear logs…
diagnostics-clear-title = Clear ordin's logs?
diagnostics-clear-body = This deletes ordin's log files and empties the list of events below. A file saved for support still includes earlier events.
diagnostics-clear-verb = Clear logs
diagnostics-clear-done = Logs cleared.
diagnostics-journal-none = No events to show.
diagnostics-col-reference = Reference
diagnostics-col-reference-hint = A short code for this event. Quote it to support so they can find it.
diagnostics-col-time = Time
diagnostics-col-level = Level
diagnostics-col-summary = What happened
diagnostics-col-details = Details
diagnostics-line-more = Show the details
diagnostics-line-less = Hide the details

# ---- Settings: Agents and models ---------------------------------------------------------------------
# SM-AGM-1 (amended January 2026; user decision; Approved by the user January 2026): the person's own
# choice of the tool that runs their agents, the model for each level and the effort for each kind of work.
settings-section-agents-and-models = Agents and models
settings-agents-intro = Choose what runs your agents and how hard they think. Anything you leave on the default follows your organisation's choice.
settings-agents-group-runs = What runs your agents
settings-agents-runs-with = Run my agents with
settings-agents-runs-default = Same as my organisation ({ $name })
settings-agents-runs-hint = A change here starts the next time you open Ordin.
settings-agents-runs-now = Your agents run with { $name } now.
settings-agents-name-claude-code = Claude Code
settings-agents-name-copilot = Copilot
settings-agents-state-needs-set-up = Needs setting up
settings-agents-state-not-found = Not found on this computer
settings-agents-state-not-in-this-version = Not available in this version yet
settings-agents-group-models = Models for { $name }
settings-agents-models-hint = Type the name of the model to use. Leave a box empty to use the default shown in it.
settings-agents-level-smaller = Quick, everyday work
settings-agents-level-standard = Most work
settings-agents-level-most-capable = The hardest work
settings-agents-model-placeholder-none = Chosen by { $name }
settings-agents-use-default = Use the default
settings-agents-model-invalid = Use letters, numbers, dots, dashes and underscores only, up to 100 characters.
settings-agents-group-effort = Effort for { $name }
settings-agents-effort-hint = More effort gives more careful answers but uses more of your plan. If you let it decide, your thinking choice in Using your plan applies when it is set.
settings-agents-effort-none = { $name } doesn't offer an effort setting, so it decides for itself.
settings-agents-effort-default = Let { $name } decide
settings-agents-kind-talking = Talking with you
settings-agents-kind-coordinating = Coordinating a crew
settings-agents-kind-routine = Routine steps
settings-agents-kind-documents = Working on documents
settings-agents-kind-code = Working on code
settings-agents-effort-low = Low
settings-agents-effort-medium = Medium
settings-agents-effort-high = High
settings-agents-effort-xhigh = Very high
settings-agents-effort-max = Highest
