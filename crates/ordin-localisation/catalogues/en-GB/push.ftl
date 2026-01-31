# The Push Service's approval card and copy header (PushService.md Internal Logic A.6 and E.2).
# One origin named: the card asks about that place.
push-card-question-one = { $count ->
    [one] Send { $count } change back to { $origin }?
   *[other] Send { $count } changes back to { $origin }?
}
# Several origins: the card does not name them in the question.
push-card-question-many = { $count ->
    [one] Send { $count } change back to where it came from?
   *[other] Send { $count } changes back to where they came from?
}
push-card-verb = Send back
# $has_branch is "yes" when the copy follows a branch, otherwise "no".
push-copy-header = { $has_branch ->
    [yes] Published copy of { $name }, from { $from }, branch { $branch }. Last updated { DATETIME($at, style: "datetime") }. Make changes in the original.
   *[no] Published copy of { $name }, from { $from }. Last updated { DATETIME($at, style: "datetime") }. Make changes in the original.
}
push-copy-audience-unknown = { $place }: everyone who can open it
