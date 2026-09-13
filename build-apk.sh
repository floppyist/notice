#!/usr/bin/env bash
# Baut Notice als native Android-App (APK).
#
# Voraussetzungen:  Rust (rustup + cargo), Android SDK + NDK (sdkmanager), cargo-ndk.
#   - ANDROID_HOME     default: $HOME/android-sdk
#   - ANDROID_NDK_HOME default: höchstes Verzeichnis unter $ANDROID_HOME/ndk
#   - JDK              Gradle 8.7 läuft auf Java 17-22. Wenn kein passendes JDK
#                      gefunden wird (z. B. nur Java 25 installiert), lädt das
#                      Skript automatisch JDK 21 nach ~/.cache/notice-jdk/.
#
# Ausgabe: android/app/build/outputs/apk/debug/app-debug.apk
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

java_major() {
  "$1" -version 2>&1 | awk -F'"' '/version/ { split($2, p, "."); if (p[1] == "1") print p[2]; else print p[1] }'
}

jdk_ok() {
  local b="$1"
  [ -x "$b" ] || return 1
  local m; m="$(java_major "$b")"
  [ "$m" -ge 17 ] && [ "$m" -le 22 ]
}

# ---- Android SDK / NDK -------------------------------------------------------
export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  NDK_DIR="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
  [ -n "$NDK_DIR" ] || { echo "Kein NDK unter \$ANDROID_HOME/ndk gefunden." >&2; exit 1; }
  export ANDROID_NDK_HOME="$NDK_DIR"
fi

# ---- JDK 17-22 suchen, sonst nachladen ----------------------------------------
JAVA_BIN=""
if [ -n "${JAVA_HOME:-}" ] && jdk_ok "$JAVA_HOME/bin/java"; then
  JAVA_BIN="$JAVA_HOME/bin/java"
fi
if [ -z "$JAVA_BIN" ]; then
  for j in "$HOME"/jdk17/bin/java "$HOME"/jdk21/bin/java \
           /usr/lib/jvm/java-17-openjdk*/bin/java \
           /usr/lib/jvm/java-21-openjdk*/bin/java \
           /usr/lib/jvm/temurin-17-jdk*/bin/java \
           /usr/lib/jvm/temurin-21-jdk*/bin/java; do
    jdk_ok "$j" && { JAVA_BIN="$j"; break; }
  done
fi
if [ -z "$JAVA_BIN" ]; then
  echo "Kein JDK 17-22 gefunden - lade JDK 21 nach ~/.cache/notice-jdk/ herunter (~190 MB)..."
  JH="$HOME/.cache/notice-jdk"
  mkdir -p "$JH"
  curl -fsSL -o "$JH/jdk21.tar.gz" \
    https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse
  tar xzf "$JH/jdk21.tar.gz" -C "$JH" && rm -f "$JH/jdk21.tar.gz"
  JAVA_BIN="$(printf '%s/bin/java' "$JH"/jdk-21.*)"
fi
jdk_ok "$JAVA_BIN" || { echo "Kein lauffähiges JDK gefunden: $JAVA_BIN" >&2; exit 1; }
export JAVA_HOME="$(dirname "$(dirname "$JAVA_BIN")")"

command -v rustc >/dev/null || { echo "Rust nicht installiert." >&2; exit 1; }
command -v cargo-ndk >/dev/null || { echo "cargo-ndk fehlt (cargo install cargo-ndk)." >&2; exit 1; }

echo "ANDROID_HOME = $ANDROID_HOME"
echo "NDK          = $ANDROID_NDK_HOME"
echo "JAVA_HOME    = $JAVA_HOME"
echo "JAVA_VERSION = $(java_major "$JAVA_BIN")"

# ---- Rust-Ziel + Cross-Compile ------------------------------------------------
rustup target add aarch64-linux-android >/dev/null 2>&1 || true

echo "==> Cross-Compile (arm64-v8a)"
cargo ndk -t arm64-v8a build --release
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/notice \
   android/app/src/main/jniLibs/arm64-v8a/libnotice.so

# ---- Gradle / APK --------------------------------------------------------------
echo "sdk.dir=$ANDROID_HOME" > android/local.properties

cd android
if [ -x ./gradlew ]; then
  GRADLE="./gradlew"
else
  command -v gradle >/dev/null || { echo "Gradle fehlt - ./gradlew nicht vorhanden." >&2; exit 1; }
  GRADLE="gradle"
fi

echo "==> assembleDebug"
"$GRADLE" --no-daemon assembleDebug

echo ""
echo "APK: $ROOT/android/app/build/outputs/apk/debug/app-debug.apk"