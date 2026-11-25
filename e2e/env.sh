# Environment for Maestro runs. Source it: `. e2e/env.sh`.
#
# - Maestro is looked for in $MAESTRO_HOME (default ~/.maestro/bin) and needs a JDK 17+.
# - `flutter` must not be on the PATH: Maestro runs it at start up and it can hang for minutes.
#   So the PATH is cut down to the JDK, adb and the system.
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@21}"
export MAESTRO_CLI_NO_ANALYTICS=1
export MAESTRO_DISABLE_UPDATE_CHECK=true
export PATH="${MAESTRO_HOME:-$HOME/.maestro}/bin:$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:/usr/bin:/bin:/usr/sbin:/sbin"
