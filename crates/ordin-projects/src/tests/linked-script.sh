#!/bin/sh
# A stand-in program whose commands are in the file `<its own name>.body` beside the link it runs through
# (standin.rs `linked_script`).
. "$(dirname "$0")/$(basename "$0").body"
