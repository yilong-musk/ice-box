import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

// Cargo release-profile overrides for the Android Rust library.
val RELEASE_PROFILE_DEFAULTS = mapOf(
    "CARGO_PROFILE_RELEASE_OPT_LEVEL" to "s",
    "CARGO_PROFILE_RELEASE_LTO" to "fat",
)

open class BuildTask : DefaultTask() {
    @Input
    var rootDirRel: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        val executable = """npm""";
        try {
            runTauriCli(executable)
        } catch (e: Exception) {
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                // Try different Windows-specific extensions
                val fallbacks = listOf(
                    "$executable.exe",
                    "$executable.cmd",
                    "$executable.bat",
                )
                
                var lastException: Exception = e
                for (fallback in fallbacks) {
                    try {
                        runTauriCli(fallback)
                        return
                    } catch (fallbackException: Exception) {
                        lastException = fallbackException
                    }
                }
                throw lastException
            } else {
                throw e;
            }
        }
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val args = listOf("run", "--", "tauri", "android", "android-studio-script");

        project.exec {
            workingDir(File(project.projectDir, rootDirRel))
            executable(executable)
            args(args)
            if (project.logger.isEnabled(LogLevel.DEBUG)) {
                args("-vv")
            } else if (project.logger.isEnabled(LogLevel.INFO)) {
                args("-v")
            }
            if (release) {
                args("--release")
                // Optimize the Android Rust library for size. It is UI glue
                // and config generation, not a hot path, and the desktop
                // release profile in Cargo.toml stays speed-optimized. LTO
                // cannot be set per package in Cargo.toml, so the override
                // goes through Cargo's profile env vars. Setting it here,
                // not in a shell script, covers every Android release build
                // (scripts/build-android-apk.sh, `tauri android build`,
                // Android Studio). A value the caller already exported wins.
                for ((name, value) in RELEASE_PROFILE_DEFAULTS) {
                    if (System.getenv(name) == null) {
                        environment(name, value)
                    }
                }
            }
            args(listOf("--target", target))
        }.assertNormalExitValue()
    }
}