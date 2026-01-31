run-progress-documents-read = { $count ->
    [one] Read one document
   *[other] Read { NUMBER($count) } documents
}
run-progress-finished-at = Finished on { DATETIME($at, style: "date") }.
run-progress-amount = The total is { NUMBER($amount) }.
    .title = Total amount
