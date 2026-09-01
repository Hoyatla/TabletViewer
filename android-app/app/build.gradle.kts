plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.nextos.screenviewer"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.nextos.screenviewer"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("com.google.android.material:material:1.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")

    // USB device enumeration (no third-party: Android USB host API is in SDK)
    // implementation("android.hardware.usb:usb:1.0")

    // HTTP server (uncomment in Phase 5)
    // implementation("org.nanohttpd:nanohttpd:2.3.1")
    // implementation("org.nanohttpd:nanohttpd-websocket:2.3.1")
}
