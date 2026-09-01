plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.nextos.screenviewer"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.nextos.screenviewer"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    signingConfigs {
        // The release keystore. Generated locally; the file itself is
        // .gitignored. For production, generate your own with
        //   keytool -genkey -v -keystore release.keystore \
        //           -keyalg RSA -keysize 2048 -validity 10000 -alias pcdebug
        // and set KEYSTORE_FILE, KEYSTORE_PASS, KEY_ALIAS, KEY_PASS env vars
        // (or hardcode the path below for dev convenience).
        create("release") {
            val ksFile = System.getenv("KEYSTORE_FILE")
                ?: "${rootDir}/app/release.keystore"
            val ksPass = System.getenv("KEYSTORE_PASS") ?: "changeit"
            val keyAlias = System.getenv("KEY_ALIAS") ?: "pcdebug"
            val keyPass = System.getenv("KEY_PASS") ?: "changeit"
            storeFile = file(ksFile)
            storePassword = ksPass
            this.keyAlias = keyAlias
            keyPassword = keyPass
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("release")
        }
        debug {
            // No special config; AGP applies the debug signing config by default.
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
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.4")

    // HTTP client for talking to pc-agent over LAN
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
}
