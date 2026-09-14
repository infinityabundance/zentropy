#!/bin/sh
# Watch a silent long run and report that it is alive.
#
# The scored stub carries no progress machinery, on purpose: `progress` is
# outside `accepted`, so the judged binary prints nothing and costs no bytes for
# it. That is the right call for `S` and it is a bad experience for anyone
# watching a 50-minute pass, which is a real problem with a real fix — observe
# the process from outside instead of making the artefact pay for the display.
#
# Reports, every `INTERVAL` seconds:
#   elapsed wall time, RSS, cumulative CPU time, and thread count.
# Exits when the process does, printing its exit status.
#
# Usage: tools/watch_run.sh [process-name-substring] [interval-seconds]
#   default: match "zentropy"  , interval 30
#
# Examples:
#   tools/watch_run.sh                       # watch any zentropy process
#   tools/watch_run.sh zentropy-sfx 15       # watch the scored stub specifically
set -eu

PATTERN="${1:-zentropy}"
INTERVAL="${2:-30}"

found=0
while :; do
    # `ps` rather than /proc so this works the same under any shell here.
    line=$(ps -eo pid=,rss=,etime=,time=,nlwp=,comm=,args= 2>/dev/null \
        | grep -- "$PATTERN" \
        | grep -v grep \
        | grep -v watch_run \
        | head -1 || true)

    if [ -z "$line" ]; then
        if [ "$found" -eq 1 ]; then
            echo "[watch] process gone — run finished or was killed"
            exit 0
        fi
        echo "[watch] no process matching '$PATTERN' yet; waiting ${INTERVAL}s ..."
        sleep "$INTERVAL"
        continue
    fi
    found=1

    pid=$(echo "$line" | awk '{print $1}')
    rss=$(echo "$line" | awk '{print $2}')
    etime=$(echo "$line" | awk '{print $3}')
    ctime=$(echo "$line" | awk '{print $4}')
    nlwp=$(echo "$line" | awk '{print $5}')

    # RSS in GiB, one decimal: enough to see a model fill up and stay there.
    gib=$(awk -v k="$rss" 'BEGIN { printf "%.2f", k / 1048576 }')
    printf '[watch] pid=%s elapsed=%s cpu=%s rss=%sGiB threads=%s\n' \
        "$pid" "$etime" "$ctime" "$gib" "$nlwp"

    sleep "$INTERVAL"
done
