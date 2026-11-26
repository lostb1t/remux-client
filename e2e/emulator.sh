#!/bin/bash
# The Android emulator for the flows, on a machine with KVM (CI, a Linux box): a TV or a phone, as
# E2E_KIND says (tv by default).
#
#   e2e/emulator.sh start    starts it (its AVD, `tv` or `phone`, is made when it is not there) and
#                            waits until it has booted
#   e2e/emulator.sh stop
#
# The emulator gets 4 cores and 3 GB: with the picture drawn in software (there is no GPU) the
# default of 2 cores leaves it too busy to answer adb now and then.
#
# Environment: E2E_KIND (tv or phone), AVD_IMAGE (default: the TV's or the phone's image below),
# EMULATOR_LOG (default /tmp/emulator.log; it is added to).
set -euo pipefail
kind="${E2E_KIND:-tv}"
case "$kind" in
  tv) default_image="system-images;android-36;android-tv;x86_64"; device=tv_1080p ;;
  phone) default_image="system-images;android-36;google_apis;x86_64"; device=pixel_6 ;;
  *) echo "E2E_KIND is tv or phone" >&2; exit 2 ;;
esac
image="${AVD_IMAGE:-$default_image}"
# Where the AVDs are, the same for avdmanager and the emulator (a newer avdmanager writes them
# elsewhere when nothing says, and the emulator then does not find them); CI keeps this folder.
export ANDROID_AVD_HOME="${ANDROID_AVD_HOME:-$HOME/.android/avd}"
mkdir -p "$ANDROID_AVD_HOME"
log="${EMULATOR_LOG:-/tmp/emulator.log}"

case "${1:-start}" in
  start)
    if ! avdmanager list avd | grep -q "Name: $kind\$"; then
      echo no | avdmanager create avd --force --name "$kind" --device "$device" --package "$image" \
        --path "$ANDROID_AVD_HOME/$kind.avd"
    fi
    # A session of its own (setsid): the emulator has to outlive the step that started it, and
    # nothing that cleans up after a step may take it along. How it ends goes in its log.
    # The adb server first: it makes the key (~/.android/adbkey) the emulator gives the device as
    # it boots. Started after the emulator, it made a key of its own now and then, and the device
    # then answered it with "unauthorized".
    adb start-server >/dev/null 2>&1
    SNAPSHOT="-no-snapshot"
    AVD="$kind" SNAPSHOT="$SNAPSHOT" setsid nohup bash -c '"$ANDROID_HOME/emulator/emulator" -avd "$AVD" $SNAPSHOT -no-window -no-audio -no-boot-anim \
      -gpu swiftshader_indirect -accel on -cores 4 -memory 3072; echo "emulator ended with status $? at $(date -u +%T)"' \
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
