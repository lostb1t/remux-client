#!/bin/bash
# Runs the Maestro flows in e2e/flows on an Android TV emulator (or device).
#
#   e2e/run.sh [flow-name ...]
#
# Environment:
#   ADB_DEVICE            the device (default: the only one connected)
#   REMUX_TEST_SERVER     the server as the device reaches it, e.g. https://jellyfin.example.com
#                         (or http://10.0.2.2:3000 for one on the machine of an emulator)
#   REMUX_TEST_USERNAME   and
#   REMUX_TEST_PASSWORD   a user to sign in as, at the login page
# Without these the app is left as it is signed in.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/env.sh"

device_args=()
[ -n "${ADB_DEVICE:-}" ] && device_args=(-s "$ADB_DEVICE")
adbd() { adb "${device_args[@]}" "$@"; }

# Remux's WebView only told Maestro what was on the page with accessibility_enabled on. NSH's
# views are Android's own, which Maestro sees without it; and with it on, Android keeps the focus
# in a text field that the app moves it from (a search sent). Off, as the TV is.
adbd shell settings put secure accessibility_enabled 0

# Signing in: at the app's login page, as a user does (lib/login.yaml), when a server, a user and a
# password are given; it does nothing when the app is signed in already. (Remux wrote the session
# to a file of its own; NSH keeps it in its storage, which a release build does not let adb write.)
# Without them, the app is used as it is signed in: a build with src/config.rs signs itself in.
if [ -n "${REMUX_TEST_SERVER:-}" ] && [ -n "${REMUX_TEST_USERNAME:-}" ] && [ -n "${REMUX_TEST_PASSWORD:-}" ]; then
  echo "== sign in"
  # shellcheck disable=SC2086
  maestro ${ADB_DEVICE:+--device "$ADB_DEVICE"} test \
    -e SERVER="$REMUX_TEST_SERVER" -e USERNAME="$REMUX_TEST_USERNAME" -e PASSWORD="$REMUX_TEST_PASSWORD" \
    "$here/lib/login.yaml" </dev/null
fi

flows=("$@")
[ ${#flows[@]} -eq 0 ] && flows=("$here"/flows/*.yaml) || flows=("${flows[@]/#/$here/flows/}")

# Waits until the device answers (up to ~90 seconds): the connection to an emulator drops now and
# then (`device offline`, `not found`) and is back a few seconds later.
wait_for_device() {
  for _ in $(seq 1 18); do
    [ "$(adbd get-state 2>/dev/null || true)" = device ] && return 0
    adbd reconnect offline >/dev/null 2>&1 || true
    sleep 5
  done
  return 1
}

status=0
restarts=0
# Every start of Maestro would put its driver on the device again, which is heavy for an emulator
# (the connection to it drops while it is busy). The first flow does; the others go on with the
# one that is there, and so does the one after a flow that had to be run again.
driver_flag=""
for flow in "${flows[@]}"; do
  [[ "$flow" == *.yaml ]] || flow="$flow.yaml"
  echo "== $(basename "$flow")"
  attempts=0
  while :; do
    if ! wait_for_device; then
      echo "the device is gone (state: $(adbd get-state 2>&1 || true)): the emulator stopped" >&2
      # On a machine where the emulator is ours to start (E2E_RESTART_EMULATOR=1) it is started
      # again: what was installed and signed in stays, in the data of the emulator.
      if [ -n "${E2E_RESTART_EMULATOR:-}" ] && [ "$restarts" -lt 3 ] && "$here/emulator.sh" start; then
        restarts=$((restarts + 1))
      else
        status=1
        break 2
      fi
    fi
    output="$(mktemp)"
    code=0
    # shellcheck disable=SC2086
    maestro ${ADB_DEVICE:+--device "$ADB_DEVICE"} test $driver_flag "$flow" </dev/null >"$output" 2>&1 || code=$?
    cat "$output"
    if [ "$code" -eq 0 ]; then
      rm -f "$output"
      driver_flag="--no-reinstall-driver"
      break
    fi
    # Only when the connection to the device was what failed, and not a check of the flow: again.
    # (What went wrong is in the log of Maestro, whose folder is named in its output, more than in
    # the output.)
    debug="$(grep -oE '/[^ ]*/\.maestro/tests/[0-9_-]+' "$output" | tail -1 || true)"
    if [ "$attempts" -lt 2 ] && cat "$output" ${debug:+"$debug/maestro.log"} 2>/dev/null \
        | grep -qE "device offline|device '.*' not found|DeviceServerDied|StatusRuntimeException: UNAVAILABLE"; then
      attempts=$((attempts + 1))
      driver_flag=""
      echo "-- the connection to the device dropped: the flow is run again ($attempts)" >&2
      rm -f "$output"
      continue
    fi
    rm -f "$output"
    status=1
    break
  done
done
adbd shell settings put secure accessibility_enabled 0
exit $status
