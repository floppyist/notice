# Notice für Android

`android/` enthält die **native** Android-App (Gradle-Projekt, Kotlin + Jetpack Compose, ohne
Material-Bibliothek – eigene eckige Komponenten im Stil von `../DESIGN.md`).
`MainActivity` startet den cross-kompilierten Rust-Server (als `libnotice.so` verpackt, aber ein
ausführbares ELF-Binary) über `ProcessBuilder` (`Server.kt`) und spricht danach nativ die REST-API
auf `http://127.0.0.1:<freier Port>` an (`Api.kt`, `Store.kt`). Es gibt keine WebView mehr.

Quellen unter `app/src/main/kotlin/de/flo/notice/`: `Theme.kt` (Tokens + Komponenten), `App.kt`
(Navigation), `Dashboard.kt`, `Board.kt`, `CalendarScreen.kt`, `WikiScreens.kt`, `Editors.kt`,
`MoreScreens.kt`. Texte: `res/values/strings.xml` (Deutsch) und `res/values-en/strings.xml`.
Schrift: `res/font/jbm_*.ttf` (Subset, erzeugt wie `../support/make_font.sh`).

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
- JDK 17–22 für Gradle (`JAVA_HOME`) – fehlt ein passendes JDK (z. B. nur Java 25),
  lädt `build-apk.sh` automatisch JDK 21 nach `~/.cache/notice-jdk/`
- Rust mit Target `aarch64-linux-android` und `cargo-ndk`
- Gradle wird automatisch über den Wrapper bezogen (Gradle 8.7, AGP 8.5.2, Kotlin 2.0.20, Compose BOM 2024.09)

`libnotice.so` wird nicht in Git eingecheckt – es entsteht beim Build.

## Debugging

- Server-Log: `filesDir/notice.log` (auf dem Gerät:
  `adb shell run-as de.flo.notice cat files/notice.log`)
- UI-Logs: `adb logcat -s Notice AndroidRuntime`