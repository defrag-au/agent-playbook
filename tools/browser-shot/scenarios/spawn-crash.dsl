# A process that dies is a failure at the wait, with its log — not a mysterious
# second symptom three steps later.
#
# This run is expected to FAIL: exit 2, a `problem` line in run.txt, and the
# command's own message quoted beneath it. It is here so that the fast-fail path
# is exercised rather than assumed.

run timeout=30s artifacts=.tmp/spawn-crash

peer page url=about:blank viewport=390x844

spawn broken command="echo 'bind: address already in use' >&2; exit 1"
wait_for log=broken pattern=listening timeout=10s
