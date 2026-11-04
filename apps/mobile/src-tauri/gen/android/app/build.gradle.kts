import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// Release signing uses the project keystore. The file and passwords stay
// outside the repository (CI secrets, or a local path for a release build).
// Debug builds keep the Android debug key and ignore these variables.
val releaseKeystorePath = System.getenv("ICE_BOX_ANDROID_KEYSTORE").orEmpty().trim()
val releaseStorePassword = System.getenv("ICE_BOX_ANDROID_KEYSTORE_PASSWORD").orEmpty()
val releaseKeyAlias = System.getenv("ICE_BOX_ANDROID_KEY_ALIAS").orEmpty().trim()
val releaseKeyPassword = System.getenv("ICE_BOX_ANDROID_KEY_PASSWORD").orEmpty()
val releaseSigningReady =
    releaseKeystorePath.isNotEmpty() &&
        releaseStorePassword.isNotEmpty() &&
        releaseKeyAlias.isNotEmpty() &&
        releaseKeyPassword.isNotEmpty()

android {
    compileSdk = 36
    namespace = "com.yilongmusk.icebox"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.yilongmusk.icebox"
        minSdk = 34
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    if (releaseSigningReady) {
        signingConfigs {
            create("release") {
                storeFile = file(releaseKeystorePath)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }
    buildTypes {
        getByName("debug") {
            applicationIdSuffix = ".debug"
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (releaseSigningReady) {
                signingConfig = signingConfigs.getByName("release")
            }
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
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
    buildFeatures {
        buildConfig = true
    }
    // The APK ships from GitHub Releases, not Google Play, so users download
    // the raw file. With minSdk >= 23 AGP stores .so files uncompressed for
    // mmap loading, which roughly triples the download (libbox.so alone is
    // ~62 MB raw, ~20 MB deflated). Compress them and let the installer
    // extract them instead. Revisit this if the app moves to Play, which
    // compresses the transfer itself. Tradeoff: the installer extracts the
    // ~71 MB of libraries next to the ~28 MB APK, so the installed app takes
    // ~98 MB instead of ~76 MB, while the download drops by ~48 MB.
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
    // Native resources are only the launcher/notification strings; the UI
    // is localized in the WebView. Drop the dozens of locales that
    // AndroidX/Material pull into resources.arsc. A bare "zh" matches no
    // library resources (they are all region-qualified), so list them.
    // Those libraries ship no other Chinese regions, so zh-SG/zh-MO devices
    // resolve exactly as before (Android 7+ falls back by script: Hans to
    // zh-CN, Hant to zh-HK/zh-TW). Other languages lose the libraries'
    // system strings (e.g. "Navigate up", TalkBack labels) and fall back to
    // English, matching the app UI, which is English and Chinese only.
    // scripts/verify-android-apk.sh --release checks this list.
    androidResources {
        localeFilters += listOf("en", "zh-rCN", "zh-rTW", "zh-rHK")
    }
    sourceSets {
        getByName("main") {
            // `third_party/sing-geoip/rule-set/*.srs` is fetched, not committed.
            // Assets are addressed as `rule-set/<file>` from that parent directory.
            assets.srcDir(file("../../../../../../third_party/sing-geoip"))
        }
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation(fileTree("libs") { include("*.aar") })
    implementation("rustls:rustls-platform-verifier:0.1.1")
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")

tasks.named("preBuild").configure {
    doFirst {
        val dir = file("../../../../../../third_party/sing-geoip/rule-set")
        val count = dir.listFiles()?.count { it.isFile && it.name.endsWith(".srs") } ?: 0
        if (count == 0) {
            error(
                "GeoIP rule-sets are missing in $dir. Run scripts/fetch-geoip.sh before building the APK.",
            )
        }
    }
}