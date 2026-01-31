# Words of the block renderer and the approval card (ScreensModule.md: step 6 Drawing a block, step 7
# Approval cards, Message catalogue entries, and the January 2026 amendment SM-SF-1, SM-SF-6, SM-SF-8).
# Each id is defined here only; the shell's own words are in screens.ftl. `{ $coordinator }` and
# `{ $product }` are filled from the brand (ADR-041).
#
# The `block-` ids below are chosen here under SA-I18N-00's pattern: step 6 quotes their wording in prose
# (for example "Ask Ordin about this", "Show more") but lists no id, so the ids are this file's.
# SPEC-GAP-1: add them to the spec's message table.

# ---- Approval card (11 §5.5). Every `approval-` id is safety wording (ADR-109). -------------------------
approval-from-ordin = From { $product } — built from the action, not written by an agent
approval-where = Where:
approval-affects = Affects:
approval-changes = Changes:
approval-reject = Reject
approval-not-now = Not now
approval-last-line = Nothing is sent or changed outside { $product } until you choose.
approval-show-whole = Show the whole message
approval-verb-read-start = { $verb } — you've read the start
approval-decided-on = Decided on { $place }, { $time }
approval-decided-in = Decided in { $chat }, { $time }
approval-outside-company = Outside the company
approval-affects-version = version { $version }

# safety
approval-bound-version-heading = { $name } — the version this approval covers
# safety
approval-bound-version-note = Changes made to this file after the card was raised are not included.
# safety
approval-bound-version-cut = Only the start of this file is shown.
# safety
approval-bound-version-unavailable = { $product } can't show the exact version this approval covers. Reject the card if you need to see it first.

# safety
approval-approve-needs-version = Approve is off because { $product } can't show the exact version this approval covers. You can still reject it.

# safety
card-decided = Approved: { $verb }.

# ---- Blocks (11 §5.15) ----------------------------------------------------------------------------------
block-cannot-show = This part can't be shown.
report-as-of = as of { $time }
report-sources = Where these numbers come from
report-show-as-table = Show as table
telegram-message-instruction-note = This message asks { $product } to do something. It's shown as information only.

# `memory-cited-by-agent`, `memory-cited-by-an-agent` (SM-SF-6) and `slack-message-instruction-note` are
# defined in screens-fixed.ftl, which was there first; the blocks use them from there.

# Actions every kind may offer.
block-ask-about-this = Ask { $coordinator } about this
block-ask-about-these = Ask { $coordinator } about these
block-ask-about-agent = Ask { $coordinator } about this agent
block-ask-why = Ask { $coordinator } why
block-ask-more-changes = Ask for more changes
block-review = Review
block-show-more = Show more
block-close-preview = Hide
block-not-now = Not now

# What the person sends with those actions (written in the conversation as the person's own message).
block-ask-text-this = Tell me more about “{ $title }”.
block-ask-text-these = Tell me more about these documents.
block-ask-text-agent = Tell me more about the agent “{ $agent }”.
block-ask-text-figures = Tell me more about these figures.
block-ask-text-more-changes = I'd like more changes to this.
block-ask-text-progress = How is this going?
block-ask-text-why = Why can't “{ $title }” be shown?
block-reply-text = Draft a reply to { $sender }'s message in #{ $channel }.
block-reply-email-text = Draft a reply to { $sender }'s email.
block-reply-reference = Message reference: { $id }

# Titles of blocks that carry none (tab titles and one-line cards).
block-title-preview = A document
block-title-document = A document
block-title-changes = Changes
block-title-key-figures = Key figures
block-title-offer = Something to decide
block-title-crew-plan = A crew of agents

# Files and attachments.
block-file-unnamed = A file
block-attachments = Attachments
block-file-saved = Saved.
block-save-file = Save file
block-save-attachment = Save attachment
block-no-messages = No messages yet.

# Question and form.
block-answer-other = Something else
block-answer-send = Answer
block-filled-by-ordin = Filled in by { $coordinator }

# Tables, figures and sources.
block-table-search = Search this table
block-table-no-rows = No rows match.
block-figure-up = Up { $percent } on { $against }
block-figure-down = Down { $percent } on { $against }
block-figure-same = Unchanged on { $against }
block-figure-was = { $against }: { $value }
block-source-documents = { $documents }
block-source-document = { $name } (version { $version })
block-source-company-system = { $system }, read { $date } at { $time }
block-source-earlier-result = An earlier result
block-source-worked-out = Worked out by { $agent } from { $files }

# Changes.
block-changes-before = Before
block-changes-after = After
block-changes-removed = removed
block-changes-added = added

# Chat lists.
block-reply-with-ordin = Reply with { $coordinator }
block-open-in-slack = Open in Slack
block-thread-replies = { $count ->
    [one] 1 reply
   *[other] { $count } replies
}

# Email.
block-email-from = From
block-email-to = To
block-email-send = Send…
block-email-edit = Edit
block-email-save-to-drafts = Save to mail drafts
block-email-saved-to-drafts = Saved to your mail drafts.
block-email-discard = Discard
block-email-discard-title = Discard this draft?
block-email-drafted-by = Drafted by { $agent }
block-email-drafted-by-an-agent = Drafted by an agent

# Offers, checklists, agents, crews, memory.
block-offer-save-question = Save this as a workflow so you can run it again?
block-offer-update-question = Update the workflow with what changed this time?
block-offer-save = Save as workflow…
block-offer-update = Update the workflow…
block-publish-checked = Publish checked items…
block-agent-search = Search the agents
access-can-read-and-change = { $agent } can read and change { $folder }
access-can-read = { $agent } can read { $folder }
block-agent-none = No agents match.
block-estimate-light = Light use
block-estimate-medium = Medium use
block-estimate-heavy = Heavy use
block-estimate-turns = about { $min } to { $max } turns
block-estimate-day = about { $percent } of a typical day's use
block-estimate-ask-first = { $coordinator } asks before starting work of this size.
block-crew-members-heading = Agents in this crew
block-crew-member = { $agent }: { $role }
block-crew-agent-unknown = An agent
block-crew-limits-heading = Limits
block-crew-limit-rounds = Rounds: up to { $count }
block-crew-limit-time = Time: up to { $minutes } min
block-crew-limit-requests = Requests: up to { $count }
block-crew-limit-running = Agents at the same time: up to { $count }
block-crew-limit-repeat = Same failure allowed: { $count } times
block-crew-approval-points = Asks you first: { $points }
block-crew-start = Start
block-crew-started = Started
block-crew-not-startable = This plan can't be started.
block-open-knowledge = Open Knowledge

# ---- Progress in three layers (11 §5.4): the same words everywhere progress is shown --------------------
progress-now = Now: { $sentence } — step { $n } of { $total }
progress-waiting = Waiting for you: { $what }
progress-running-for = Running for { $minutes } min
progress-last-checked = last checked { $seconds } seconds ago
progress-step-done = done
progress-step-now = now
progress-step-next = next
progress-step-skipped = skipped
progress-plan-changed = Plan changed: { $what }
progress-show-what-happened = Show what happened
progress-no-activity = Done — the agent reports no activity for this step
progress-count-of = { $done } of { $total }
progress-count-so-far = { $done } so far
progress-stop-note = Stop — files written so far are kept
progress-every-week = Every week…

# ---- Ready to publish (SM-SF-8) -----------------------------------------------------------------------
ready-kind-changed = Changed since it was published
