# Approval-card wording. Every identifier beginning `approval-` is safety wording (ADR-109).
approval-card-send-button = Send
approval-card-discard-button = Don't send
approval-card-title = Approve “{ $action }”?
approval-card-who-acts = { $actor } will carry this out once you approve.

# Card words for a Rules Engine proposal and a crew step (OrdinDesktop.md DSK-REC-1, DSK-REC-2, DSK-REC-4).
approval-rules-not-stated = Not stated
approval-rules-verb-run = Run
approval-rules-verb-allow = Allow
approval-rules-verb-finish = Finish anyway
approval-rules-where-this-computer = This computer
approval-rules-where-this-run = This run
approval-rules-where-rule = Rule { $rule }
approval-rules-shell-question = Run this command in { $runs_in }?
approval-rules-shell-changes = The command can read and change files in { $runs_in }.
approval-rules-external-question = Let the agent use { $tool } to reach { $destination }?
approval-rules-external-changes = Information leaves this computer for { $destination }.
approval-rules-package-question = Let the agent use { $tool } from { $server }?
approval-rules-package-changes = A tool from the agent's package runs with what { $server } can reach.
approval-rules-listed-question = Let the agent use { $tool }?
approval-rules-listed-changes = Your company asks you to approve each use of { $tool }.
approval-rules-argument-question = Let the agent use { $tool } as it asked?
approval-rules-argument-changes = The rule { $rule } holds this call until you check it.
approval-rules-policy-question = Let the agent { $verb } with { $tool }?
approval-rules-policy-changes = This agent's settings ask for your approval before it can { $verb }.
approval-rules-gates-question = Finish without the checks that did not pass?
approval-rules-gates-changes = The run ends although these checks did not pass: { $gates }.
approval-crew-step-question = Carry on with “{ $step }”?
approval-crew-verb-carry-on = Carry on
approval-crew-where = Crew: { $purpose }
approval-crew-step-changes = The crew waits here until you choose.
