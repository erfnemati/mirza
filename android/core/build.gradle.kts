import javax.inject.Inject

// Mirza's Rust core (crates/mobile) for the apps: the native library for each ABI,
// its Kotlin bindings (package dev.erfnemati.mirza.core) and JNA, which they call through.
plugins {
    id("com.android.library")
}

// `-Pmirza.rustProfile=dev` for faster native builds while working on Rust,
// `-Pmirza.abis=arm64-v8a` to build for phones only (x86_64 is the emulator).
val rustProfile = providers.gradleProperty("mirza.rustProfile").getOrElse("release")
val rustAbis = providers.gradleProperty("mirza.abis").getOrElse("arm64-v8a,x86_64").split(",").map { it.trim() }

android {
    namespace = "dev.erfnemati.mirza.core"
    compileSdk = 36

    defaultConfig {
        minSdk = 29
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // UniFFI's Kotlin bindings call the Rust library through JNA.
    api("net.java.dev.jna:jna:5.19.1@aar")
}

abstract class BuildRust : DefaultTask() {
    @get:Input abstract val profile: Property<String>
    @get:Input abstract val abis: ListProperty<String>
    @get:Internal abstract val script: RegularFileProperty
    @get:OutputDirectory abstract val jniLibs: DirectoryProperty
    @get:OutputDirectory abstract val bindings: DirectoryProperty
    @get:Inject abstract val exec: ExecOperations

    @TaskAction
    fun build() {
        exec.exec {
            commandLine(
                listOf(script.get().asFile.path, profile.get(), jniLibs.get().asFile.path, bindings.get().asFile.path) +
                    abis.get(),
            )
        }
    }
}

val buildRust = tasks.register<BuildRust>("buildRust") {
    profile = rustProfile
    abis = rustAbis
    script = rootProject.layout.projectDirectory.file("build-rust.sh")
    jniLibs = layout.buildDirectory.dir("rust/jniLibs")
    bindings = layout.buildDirectory.dir("rust/kotlin")
    // cargo knows best what changed.
    outputs.upToDateWhen { false }
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(buildRust, BuildRust::jniLibs)
        variant.sources.kotlin?.addGeneratedSourceDirectory(buildRust, BuildRust::bindings)
    }
}
