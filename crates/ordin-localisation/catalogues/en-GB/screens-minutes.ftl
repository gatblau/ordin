## The meeting minutes screens (ScreensModule.md SM-MIN-1 to SM-MIN-10). Drawn only by a build with the
## `minutes` switch. OBS menu names are OBS's own labels.

minutes-tree = Minutes
minutes-heading = Minutes
minutes-import = Make minutes from a file…
minutes-record = Record
minutes-cancel = Cancel
minutes-retry = Try again

minutes-stage-queued = Waiting to start
minutes-stage-waiting-for-model = Waiting for the speech model
minutes-stage-downloading-model = Downloading the speech model — { $received } of { $total } MB
minutes-stage-reading = Reading the transcript
minutes-stage-transcribing = Turning the recording into text — { $done } of { $total } minutes
minutes-stage-waiting-for-notice = Waiting for your OK
minutes-stage-writing = Ordin is writing the minutes
minutes-stage-transcript-ready = Ready for Ordin
# SM-MIN-11: a job that turns a recording into text for Ordin.
minutes-job-for-ordin = text for Ordin

minutes-model-title = Download Ordin's speech model?
minutes-model-body = To turn recordings into text on this computer, Ordin needs a speech model of { $size } MB. It is saved in { $folder } and used only on this computer. Transcript files work without it.
minutes-model-download = Download
minutes-model-not-now = Not now

minutes-notice-title = Before Ordin writes minutes
minutes-notice-body = Ordin sends the text of the meeting — never the recording — to { $service } to write the minutes. This is the AI service your agents already use.
minutes-notice-accept = Write the minutes

minutes-record-title = Tell everyone you're recording
minutes-record-body = Let everyone in the meeting know that you are recording before you start.
minutes-record-start = Start recording
minutes-record-mic-only = Only your microphone will be recorded. Update OBS to version 30 or later to record what others say.

minutes-get-obs = Get OBS
minutes-open-obs = Open OBS
minutes-obs-password = OBS password
minutes-obs-save = Save

minutes-recording = Recording — { $time }
minutes-recording-started = Recording started.
minutes-recording-stopped = Recording stopped.
minutes-stop = Stop
minutes-ready = Minutes of { $title } are ready.

minutes-kept-on = Made from { $source } on { $date }
minutes-save = Save
minutes-saved = Minutes saved.
minutes-reopen = Reopen
minutes-export = Export
minutes-export-pdf = PDF…
minutes-export-word = Word…
minutes-exported = Minutes exported.
minutes-show-transcript = Show transcript
minutes-delete-recording = Delete recording…
minutes-delete-recording-title = Delete the recording?
minutes-delete-recording-body = The minutes and the transcript stay.
minutes-delete-recording-verb = Delete recording
minutes-remove = Remove…
minutes-remove-title = Remove { $title }?
minutes-remove-body = The minutes, their transcript and their recording are deleted from this computer.
minutes-remove-verb = Remove

settings-section-meeting-minutes = Meeting minutes
minutes-settings-model = Speech model
minutes-settings-model-larger = Larger — more accurate, about 1.5 GB
minutes-settings-model-smaller = Smaller — faster, about 470 MB
minutes-settings-model-downloaded = Downloaded
minutes-settings-model-not-downloaded = Not downloaded yet
minutes-model-recommended = Recommended for this computer
minutes-model-recommended-line = { $model } — the one this computer handles well
minutes-model-name-larger = Larger, about 1.5 GB
minutes-model-name-smaller = Smaller, about 470 MB
minutes-model-larger-slow = On this computer the larger model may take more than an hour for each hour of meeting.
minutes-settings-remove-model = Remove the speech model
minutes-settings-recordings = Recordings
minutes-settings-delete-after = Delete recordings after the minutes are saved
minutes-settings-recordings-kept = Recordings are kept in Ordin's folder on this computer until you delete them.
minutes-settings-obs = OBS
minutes-settings-obs-port = Port
minutes-record-menu = Record…
minutes-job-failed = Couldn't finish
minutes-obs-not-ready-title = OBS isn't ready to record
minutes-needs-model = Download the speech model for { $file }?
minutes-needs-notice = Before Ordin writes minutes of { $file }
