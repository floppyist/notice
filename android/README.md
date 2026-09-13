# Notice für Android

`android/` enthält eine native Android-Verpackung (Gradle-Projekt) der Notice-Web-App.
Eine Java-Activity mit Vollbild-WebView startet den cross-kompilierten Rust-Server des
Projekts (als `libnotice.so` verpackt, aber ein ausführbares ELF-Binary) über
`ProcessBuilder` und lädt anschließend `http://127.0.0.1:<freier Port>/`.

Daten (`config.toml`, `notice.db`, `contacts.db`, `backups/`) liegen im app-eigenen
Verzeichnis (`filesDir`) und bleiben bei Updates erhalten.

## Build

Eine Regel: `./build-apk.sh` im Projekt-Root macht alles (Rust-Cross-Compile +
Gradle-Build):

```bash
./build-apk.sh
# Ausgabe: android/app/build/outputs/apk/debug/app-debug.apk  (arm64-v8a)
```

Manuell (entspricht dem Script):

```bash
# 1) Rust-Ziel + Cross-Compile (einmalig: rustup target add aarch64-linux-android)
export ANDROID_HOME=$HOME/android-sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>
cargo ndk -t arm64-v8a build --release
cp target/aarch64-linux-android/release/notice app/src/main/jniLibs/arm64-v8a/libnotice.so

# 2) APK
echo "sdk.dir=$ANDROID_HOME" > local.properties
./gradlew assembleDebug      # oder: gradle assembleDebug
```

## Voraussetzungen

- Android SDK (cmdline-tools, platform-tools, `platforms;android-34`,
  `build-tools;34.0.0`) und NDK (`ndk;28.2.13676358`), z. B. über `sdkmanager`
- JDK 17–22 (`JAVA_HOME`) – neuere Versionen sind mit Gradle 8.7 nicht getestet
- Rust mit Target `aarch64-linux-android` und `cargo-ndk`
- Gradle wird automatisch über den Wrapper bezogen (Gradle 8.7, AGP 8.5.2)

`libnotice.so` wird nicht in Git eingecheckt – es entsteht beim Build.

## Debugging

- Server-Log: `filesDir/notice.log` (auf dem Gerät:
  `adb shell run-as de.flo.notice cat files/notice.log`)
- WebView-Debug (Chrome DevTools, `chrome://inspect`):
  `adb forward tcp:9222 localabstract:webview_devtools_remote_<pid>`