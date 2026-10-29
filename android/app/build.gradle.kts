import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "de.flo.notice"
    compileSdk = 34

    defaultConfig {
        applicationId = "de.flo.notice"
        minSdk = 26
        targetSdk = 34
        versionCode = 2
        versionName = "2.0.0"
    }

    // Release-Signatur aus android/keystore.properties (gitignoriert):
    //   storeFile=/pfad/notice-release.jks  storePassword=…  keyAlias=notice  keyPassword=…
    val ksProps = Properties().apply {
        val f = rootProject.file("keystore.properties")
        if (f.exists()) f.inputStream().use { load(it) }
    }
    signingConfigs {
        if (ksProps.containsKey("storeFile")) {
            create("release") {
                storeFile = file(ksProps.getProperty("storeFile"))
                storePassword = ksProps.getProperty("storePassword")
                keyAlias = ksProps.getProperty("keyAlias")
                keyPassword = ksProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfigs.findByName("release")?.let { signingConfig = it }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    packaging {
        jniLibs {
            // libnotice.so muss unkomprimiert auf dem Gerät liegen (wird als
            // ausführbares Binary via ProcessBuilder gestartet).
            useLegacyPackaging = true
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.09.03")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-text")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
}
