import groovy.json.JsonSlurper

buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.25")
    }
}

// The Kotlin half of rustls-platform-verifier 0.7.0 ships inside the crate.
// `cargo` must be on PATH when Gradle configures the project.
fun rustlsPlatformVerifierMaven(): String {
    val output = providers.exec {
        workingDir = rootDir
        commandLine(
            "cargo", "metadata", "--format-version", "1",
            "--filter-platform", "aarch64-linux-android",
            "--manifest-path", "../../Cargo.toml",
        )
    }.standardOutput.asText.get()
    @Suppress("UNCHECKED_CAST")
    val packages = (JsonSlurper().parseText(output) as Map<String, Any>)["packages"] as List<Map<String, Any>>
    val manifest = packages.first { it["name"] == "rustls-platform-verifier-android" }["manifest_path"] as String
    return File(manifest).parentFile.resolve("maven").path
}

val rustlsMaven = rustlsPlatformVerifierMaven()

allprojects {
    repositories {
        google()
        mavenCentral()
        maven {
            url = uri(rustlsMaven)
            metadataSources {
                mavenPom()
                artifact()
            }
        }
    }
}

tasks.register("clean").configure {
    delete("build")
}

