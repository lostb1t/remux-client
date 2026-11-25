#!/bin/sh
# Compiles the app's Java (android/src) to android/out/classes.dex, which nsh.json
# "android.libraries" ships beside the libraries of android/libs (fetch-libs.py). Needs a JDK
# (Android Studio's) and the Android SDK.
set -e
cd "$(dirname "$0")"
sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
jar="$sdk/platforms/android-36/android.jar"
tools="$sdk/build-tools/$(ls "$sdk/build-tools" | sort -V | tail -1)"
export JAVA_HOME="${JAVA_HOME:-/Applications/Android Studio.app/Contents/jbr/Contents/Home}"
[ -d libs ] && [ -n "$(ls libs)" ] || python3 fetch-libs.py
rm -rf out && mkdir -p out/classes out/classpath
# The classes of the libraries, to compile against.
for aar in libs/*.aar; do unzip -q -o -p "$aar" classes.jar > "out/classpath/$(basename "$aar" .aar).jar"; done
cp libs/*.jar out/classpath/
classpath="$jar:$(ls out/classpath/*.jar | tr '\n' ':')"
"$JAVA_HOME/bin/javac" --release 8 -nowarn -classpath "$classpath" -d out/classes $(find src -name '*.java') 2>&1 | grep -v "warning: \[options\]\|^[0-9]* warning" || true
libs=""
for j in out/classpath/*.jar; do libs="$libs --classpath $j"; done
"$tools/d8" --lib "$jar" $libs --min-api 26 --output out $(find out/classes -name '*.class')
ls out/classes.dex
