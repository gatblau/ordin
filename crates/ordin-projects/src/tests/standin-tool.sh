#!/bin/sh
# The stand-in for `gh` and `glab` (standin.rs). The address of the wiremock server is in the file
# `standin-base` beside the link this runs through.
base=$(cat "$(dirname "$0")/standin-base")
if [ "$1" != "api" ]; then
  printf 'args:%s\n' "$*"
  printf 'token:%s%s\n' "$GH_TOKEN" "$GITLAB_TOKEN"
  printf 'host:%s prompt:%s colour:%s\n' "$GITLAB_HOST" "$GH_PROMPT_DISABLED" "$NO_COLOR"
  pwd
  [ "$3" = "fail" ] && exit 3
  exit 0
fi
path=$2
shift 2
method=GET
fields=$(mktemp)
while [ $# -gt 0 ]; do
  case "$1" in
    --method) method=$2; shift 2 ;;
    --include) shift ;;
    -f) printf '%s\n' "$2" >> "$fields"; shift 2 ;;
    *) shift ;;
  esac
done
if [ -s "$fields" ]; then
  /usr/bin/curl -sS -i -X "$method" -H "Authorization: token $GH_TOKEN$GITLAB_TOKEN" \
    -H "Content-Type: text/plain" --data-binary @"$fields" "$base/$path"
else
  /usr/bin/curl -sS -i -X "$method" -H "Authorization: token $GH_TOKEN$GITLAB_TOKEN" "$base/$path"
fi
status=$?
rm -f "$fields"
exit $status
