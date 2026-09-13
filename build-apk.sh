#!/usr/bin/env bash
# Baut Notice als native Android-App (APK).
#
# Voraussetzungen:  Rust (rustup + cargo), Android SDK + NDK, JDK 17-22.
#   - ANDROID_HOME     default: $HOME/android-sdk
#   - ANDROID_NDK_HOME default: höchstes Verzeichnis unter $ANDROID_HOME/ndk
#   - JAVA_HOME        default: $HOME/jdk17 oder ein passendes JDK im System
#
# Ausgabe: android/app/build/outputs/apk/debug/app-debug.apk
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

# ---- Toolchain-Suche -------------------------------------------------------
export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  NDK_DIR="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
  [ -n "$NDK_DIR" ] || { echo "Kein NDK unter \$ANDROID_HOME/ndk gefunden." >&2; exit 1; }
  export ANDROID_NDK_HOME="$NDK_DIR"
fi

if [ -z "${JAVA_HOME:-}" ]; then
  for c in "$HOME/jdk17/bin/java" /usr/lib/jvm/java-17-openjdk-amd64/bin/java \
           /usr/lib/jvm/java-17-openjdk/bin/java /usr/lib/jvm/java-21-openjdk/bin/java; do
    [ -x "$c" ] && export JAVA_HOME="$(dirname "$(dirname "$c")")" && break
  done
  export JAVA_HOME="${JAVA_HOME:-$(dirname "$(dirname "$(command -v java)")")}"
fi
command -v rustc >/dev/null || { echo "Rust nicht installiert." >&2; exit 1; }

echo "ANDROID_HOME = $ANDROID_HOME"
echo "NDK          = $ANDROID_NDK_HOME"
echo "JAVA_HOME    = $JAVA_HOME"

# ---- Rust-Ziel + Cross-Compile ----------------------------------------------
rustup target add aarch64-linux-android >/dev/null 2>&1 || true
command -v cargo-ndk >/dev/null || { echo "cargo-ndk fehlt (cargo install cargo-ndk)." >&2; exit 1; }

echo "==> Cross-Compile (arm64-v8a)"
cargo ndk -t arm64-v8a build --release
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/notice \
   android/app/src/main/jniLibs/arm64-v8a/libnotice.so

# ---- Gradle / APK ------------------------------------------------------------
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