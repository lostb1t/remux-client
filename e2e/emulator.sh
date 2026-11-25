#!/bin/bash
# The Android TV emulator for the flows, on a machine with KVM (CI, a Linux box).
#
#   e2e/emulator.sh start    starts it (the AVD `tv` is made when it is not there) and waits until it
#                            has booted
#   e2e/emulator.sh stop
#
# The emulator gets 4 cores and 3 GB: with the picture drawn in software (there is no GPU) the
# default of 2 cores leaves it too busy to answer adb now and then.
#
# Environment: AVD_IMAGE (default system-images;android-36;android-tv;x86_64), EMULATOR_LOG
# (default /tmp/emulator.log; it is added to).
set -euo pipefail
image="${AVD_IMAGE:-system-images;android-36;android-tv;x86_64}"
log="${EMULATOR_LOG:-/tmp/emulator.log}"

case "${1:-start}" in
  start)
    if ! avdmanager list avd | grep -q 'Name: tv$'; then
      echo no | avdmanager create avd --force --name tv --device tv_1080p --package "$image"
    fi
    # A session of its own (setsid): the emulator has to outlive the step that started it, and
    # nothing that cleans up after a step may take it along. How it ends goes in its log.
    setsid nohup bash -c '"$ANDROID_HOME/emulator/emulator" -avd tv -no-window -no-audio -no-boot-anim \
      -no-snapshot -gpu swiftshader_indirect -accel on -cores 4 -memory 3072; echo "emulator ended with status $? at $(date -u +%T)"' \
      >> "$log" 2>&1 < /dev/null &
    # Up to 10 minutes.
    for _ in $(seq 1 120); do
      if [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; then
        # Just booted, it is still busy starting its services, and the connection to it drops
        # while the first flow installs its driver. Give it a minute.
        sleep 45
        adb wait-for-device
        echo "the emulator has booted"
        exit 0
      fi
      if grep -q "^emulator ended" "$log"; then
        echo "the emulator stopped:" >&2
        tail -30 "$log" >&2
        exit 1
      fi
      sleep 5
    done
    echo "the emulator did not boot in time" >&2
    tail -30 "$log" >&2
    exit 1
    ;;
  stop)
    adb emu kill >/dev/null 2>&1 || true
    ;;
  *)
    echo "usage: $0 start|stop" >&2
    exit 2
    ;;
esac
