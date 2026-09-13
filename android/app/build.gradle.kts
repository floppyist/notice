plugins {
    id("com.android.application")
}

android {
    namespace = "de.flo.notice"
    compileSdk = 34

    defaultConfig {
        applicationId = "de.flo.notice"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "1.6.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            // libnotice.so muss unkomprimiert auf dem Gerät liegen (wird als
            // ausführbares Binary via ProcessBuilder gestartet).
            useLegacyPackaging = true
        }
    }
}