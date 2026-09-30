import groovy.json.JsonSlurper

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

// rustls-platform-verifier ships its Kotlin component inside the crate; point
// a local Maven repository at the resolved crate source.
fun rustlsPlatformVerifierMaven(): String {
    val output = providers.exec {
        workingDir = rootDir
        commandLine(
            "cargo", "metadata", "--format-version", "1",
            "--filter-platform", "aarch64-linux-android",
            "--manifest-path", "rust/Cargo.toml",
        )
    }.standardOutput.asText.get()
    @Suppress("UNCHECKED_CAST")
    val packages = (JsonSlurper().parseText(output) as Map<String, Any>)["packages"] as List<Map<String, Any>>
    val manifest = packages.first { it["name"] == "rustls-platform-verifier-android" }["manifest_path"] as String
    return File(File(manifest).parentFile, "maven").path
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        maven {
            url = uri(rustlsPlatformVerifierMaven())
            metadataSources { mavenPom(); artifact() }
        }
    }
}

rootProject.name = "icebox-spike"
include(":app")
