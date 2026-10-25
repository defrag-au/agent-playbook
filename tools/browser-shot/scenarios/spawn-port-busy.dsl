# A second process on a port the first one holds is refused before it starts.
#
# This run is expected to FAIL with exit 1 and say why. The check is here rather
# than in `wait_for` because by the time a wait runs, a server that failed to
# bind has already died and the squatter is what answers — so a wait would pass
# against the wrong build.

run timeout=30s artifacts=.tmp/spawn-port-busy

peer page url=about:blank viewport=390x844

spawn first command="python3 -m http.server 8792 --bind 127.0.0.1" port=8792
wait_for ready=first timeout=20s
spawn second command="python3 -m http.server 8792 --bind 127.0.0.1" port=8792
