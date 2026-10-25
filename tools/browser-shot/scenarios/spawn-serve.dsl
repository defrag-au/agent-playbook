# A scenario can start the thing it tests, and stop it again.
#
# `spawn` owns a process group, `wait_for` decides when it is up, and `goto`
# loads a page from it — a peer's `url=` is fetched before any step runs, so a
# page served by a process this scenario started needs the explicit navigation.
#
# The teardown is the point: after this run nothing on 8791 survives, which the
# acceptance line in README.md asserts with `lsof -ti tcp:8791`.
#
# `python3 -m http.server` is a throwaway origin for the fixture, not part of the
# tool: a scenario runs whatever command it is given.

run timeout=90s artifacts=.tmp/spawn-serve

peer page url=about:blank viewport=390x844

spawn site command="python3 -m http.server 8791 --bind 127.0.0.1" port=8791 dir=tools/browser-shot/scenarios
wait_for ready=site timeout=20s
wait_for http=http://127.0.0.1:8791/page.html timeout=10s

goto page http://127.0.0.1:8791/page.html
wait page predicate="window.__ready === true" timeout=10s

assert page "document.title === 'serve fixture'" message="the page came from the spawned server"
capture page out=served.png
