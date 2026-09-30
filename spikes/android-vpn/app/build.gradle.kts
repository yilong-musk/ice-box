plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.yilongmusk.icebox.spike"
    compileSdk = 35
    buildToolsVersion = "35.0.0"
    ndkVersion = "28.0.13004108"

    defaultConfig {
        applicationId = "com.yilongmusk.icebox.spike"
        minSdk = 34
        targetSdk = 35
        versionCode = 1
        versionName = "0.0.1"
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    sourceSets["main"].assets.srcDirs("../../../third_party/sing-geoip")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation(fileTree("libs") { include("*.aar") })
    implementation("rustls:rustls-platform-verifier:0.1.1")
}
