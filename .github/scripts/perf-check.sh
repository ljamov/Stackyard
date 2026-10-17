#!/usr/bin/env bash
# Starts the installed app a few times and checks it against the budgets:
# the window's page loads within MAX_START_MS, and once the window is
# closed (tray only) the app with everything it started uses under
# MAX_TRAY_MB of memory. The app reports when each point is reached
# (STACKYARD_PERF_REPORT, see src-tauri/src/perf.rs).
#
# Usage: perf-check.sh <path to the app's program>
set -euo pipefail

app="$1"
runs="${PERF_RUNS:-5}"
max_start_ms="${MAX_START_MS:-1000}"
max_tray_mb="${MAX_TRAY_MB:-40}"
work="${RUNNER_TEMP:-/tmp}/stackyard-perf"
rm -rf "$work"
mkdir -p "$work"
export STACKYARD_HOME="$work/home"

now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'; }

# Memory the app and every process it started use, in KB, counted the way
# each system's own monitor does: memory private to the process, not the
# shared libraries mapped into it (those pages are on disk and shared).
tree_kb() {
  case "$(uname -s)" in
    Linux)
      local pids="$1" all="$1" next
      while [ -n "$pids" ]; do
        next=""
        for p in $pids; do next="$next $(pgrep -P "$p" || true)"; done
        pids=$(echo $next)
        all="$all $pids"
      done
      for p in $all; do cat "/proc/$p/smaps_rollup" 2>/dev/null; done |
        awk '/^Private_Dirty:/ { s += $2 } END { print s }' ;;
    Darwin)
      # Activity Monitor's Memory column. The web view's helpers run under
      # launchd, not the app; they are gone once the window is closed.
      vmmap --summary "$1" 2>/dev/null | awk '/^Physical footprint:/ {
        v = $3; u = substr(v, length(v)); n = v + 0
        if (u == "G") n *= 1048576; else if (u == "M") n *= 1024
        print int(n) }' ;;
    *)
      powershell -NoProfile -Command "
        \$all = Get-CimInstance Win32_Process
        \$ids = @($1); \$i = 0
        while (\$i -lt \$ids.Count) { \$ids += @(\$all | Where-Object { \$_.ParentProcessId -eq \$ids[\$i] } | ForEach-Object { \$_.ProcessId }); \$i++ }
        # Private working set, what Task Manager shows as Memory.
        \$kb = 0
        foreach (\$id in \$ids) { \$c = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter \"IDProcess=\$id\"; if (\$c) { \$kb += [math]::Round(\$c.WorkingSetPrivate / 1024) } }
        \$kb" | tr -d '\r' ;;
  esac
}

start_app() {
  case "$(uname -s)" in
    Linux|Darwin)
      "$app" > "$work/app.log" 2>&1 &
      echo $! ;;
    *)
      powershell -NoProfile -Command "(Start-Process -FilePath '$(cygpath -w "$app")' -PassThru).Id" | tr -d '\r' ;;
  esac
}

stop_app() {
  case "$(uname -s)" in
    Linux|Darwin) kill "$1" 2>/dev/null || true; sleep 1; kill -9 "$1" 2>/dev/null || true ;;
    *) taskkill //F //T //PID "$1" > /dev/null 2>&1 || true ;;
  esac
  sleep 2
}

starts=(); insides=(); trays=()
for run in $(seq "$runs"); do
  report="$work/report-$run.txt"
  export STACKYARD_PERF_REPORT="$report"
  [ "$(uname -s)" = Linux ] || [ "$(uname -s)" = Darwin ] || STACKYARD_PERF_REPORT=$(cygpath -w "$report")
  t0=$(now_ms)
  pid=$(start_app)
  for _ in $(seq 600); do grep -q '^ready_ms=' "$report" 2>/dev/null && break; sleep 0.05; done
  t1=$(now_ms)
  if ! grep -q '^ready_ms=' "$report" 2>/dev/null; then
    echo "::error::the app's window did not load within 30 s"
    cat "$work/app.log" 2>/dev/null || true
    stop_app "$pid"; exit 1
  fi
  start=$((t1 - t0))
  inside=$(sed -n 's/^ready_ms=//p' "$report")
  for _ in $(seq 60); do grep -q '^tray=1' "$report" && break; sleep 0.5; done
  grep -q '^tray=1' "$report" || { echo "::error::the app did not report tray mode"; stop_app "$pid"; exit 1; }
  kb=$(tree_kb "$pid")
  tray=$(awk "BEGIN { printf \"%.1f\", $kb / 1024 }")
  echo "run $run: window loaded in $start ms ($inside ms after main), tray only $tray MB"
  echo "  steps after main: $(grep -v '^tray=' "$report" | tr '\n' ' ')"
  stop_app "$pid"
  starts+=("$start"); insides+=("$inside"); trays+=("$tray")
done

# The middle run: steadier than any one run, and not flattered by the best.
median() { printf '%s\n' "$@" | sort -n | awk '{ v[NR] = $1 } END { print v[int((NR + 1) / 2)] }'; }
mid_start=$(median "${starts[@]}")
mid_app=$(median "${insides[@]}")
mid_tray=$(median "${trays[@]}")

{
  echo "### Performance on $(uname -s)"
  echo
  echo "| Check | Measured | Budget |"
  echo "| --- | --- | --- |"
  echo "| Start until the window has loaded | $mid_start ms ($mid_app ms after main) | $max_start_ms ms |"
  echo "| Memory with the window closed (tray only) | $mid_tray MB | $max_tray_mb MB |"
} >> "${GITHUB_STEP_SUMMARY:-/dev/stdout}"

failed=0
if [ "$mid_start" -gt "$max_start_ms" ]; then
  echo "== what the app printed on its last start"
  tail -n 30 "$work/app.log" 2>/dev/null || true
  echo "::error::Start took $mid_start ms, over the $max_start_ms ms budget"
  failed=1
fi
if awk "BEGIN { exit !($mid_tray > $max_tray_mb) }"; then
  echo "::error::Tray-only memory is $mid_tray MB, over the $max_tray_mb MB budget"
  failed=1
fi
exit "$failed"
