#!/usr/bin/env bash
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

export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  NDK_DIR="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
  [ -n "$NDK_DIR" ] || { echo "Kein NDK unter \$ANDROID_HOME/ndk gefunden." >&2; exit 1; }
  export ANDROID_NDK_HOME="$NDK_DIR"
fi

JAVA_BIN=""
if [ -n "${JAVA_HOME:-}" ] && jdk_ok "$JAVA_HOME/bin/java"; then
  JAVA_BIN="$JAVA_HOME/bin/java"
fi
if [ -z "$JAVA_BIN" ]; then
  for j in "$HOME"/jdk17/bin/java "$HOME"/jdk21/bin/java \
           /usr/lib/jvm/java-17-openjdk*/bin/java \
           /usr/lib/jvm/java-21-openjdk*/bin/java; do
    jdk_ok "$j" && { JAVA_BIN="$j"; break; }
  done
fi
if [ -z "$JAVA_BIN" ]; then
  JH="$HOME/.cache/notice-jdk"
  mkdir -p "$JH"
  if [ ! -x "$JH/jdk-21.*/bin/java" ]; then
    curl -fsSL -o "$JH/jdk21.tar.gz" \
      https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jdk/hotspot/normal/eclipse
    tar xzf "$JH/jdk21.tar.gz" -C "$JH" && rm -f "$JH/jdk21.tar.gz"
  fi
  JAVA_BIN="$(find "$JH" -name java -path '*/bin/java' | head -1)"
fi
jdk_ok "$JAVA_BIN" || { echo "Kein lauffähiges JDK gefunden" >&2; exit 1; }
export JAVA_HOME="$(dirname "$(dirname "$JAVA_BIN")")"

echo "ANDROID_HOME = $ANDROID_HOME"
echo "NDK          = $ANDROID_NDK_HOME"
echo "JAVA_HOME    = $JAVA_HOME"
echo "JAVA_VERSION = $(java_major "$JAVA_BIN")"

rustup target add aarch64-linux-android >/dev/null 2>&1 || true

export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
export CC_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/clang"
export CXX_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/clang++"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/clang"
export AR_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
export RANLIB_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ranlib"
export CFLAGS_aarch64_linux_android="--target=aarch64-linux-android21 -DANDROID"
export CXXFLAGS_aarch64_linux_android="--target=aarch64-linux-android21 -DANDROID"

echo "==> Cross-Compile (arm64-v8a)"
cargo ndk -t arm64-v8a -p 21 build --release
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/notice \
   android/app/src/main/jniLibs/arm64-v8a/libnotice.so

echo "sdk.dir=$ANDROID_HOME" > android/local.properties

cd android
if [ -x ./gradlew ]; then
  GRADLE="./gradlew"
else
  GRADLE="gradle"
fi

echo "==> assembleDebug"
"$GRADLE" --no-daemon assembleDebug

echo ""
echo "APK: $ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
