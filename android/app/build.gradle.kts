plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// One version for the whole project: the workspace's, in the root Cargo.toml.
val mirzaVersion: String = providers.fileContents(rootProject.layout.projectDirectory.file("../Cargo.toml")).asText.map {
    Regex("""(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"""").find(it)?.groupValues?.get(1)
        ?: error("no [workspace.package] version in Cargo.toml")
}.get()

// 0.1.5 -> 1005, 1.2.3 -> 1002003: always higher for a newer version.
val mirzaVersionCode: Int = mirzaVersion.substringBefore('-').split('.').map { it.toInt() }
    .let { (major, minor, patch) -> major * 1_000_000 + minor * 1_000 + patch }

// The release key (docs/android-app.md). Every build is signed with it when it can be found,
// debug builds included, so they install over each other and Google sign-in sees one
// certificate. Looked for in MIRZA_ANDROID_KEYSTORE_FILE (the Release workflow), then in
// .secrets/ of this checkout, then of the main clone when this is a git worktree.
val keystore: File? = run {
    val name = ".secrets/android-release.p12"
    val commonDir = providers.exec {
        commandLine("git", "rev-parse", "--path-format=absolute", "--git-common-dir")
        isIgnoreExitValue = true
    }.standardOutput.asText.get().trim()
    listOfNotNull(
        providers.environmentVariable("MIRZA_ANDROID_KEYSTORE_FILE").orNull?.let { File(it) },
        rootProject.file("../$name"),
        commonDir.takeIf { it.isNotEmpty() }?.let { File(it).parentFile.resolve(name) },
    ).firstOrNull { it.isFile }
}
val keystorePassword: String? = providers.environmentVariable("MIRZA_ANDROID_KEYSTORE_PASSWORD").orNull
    ?: keystore?.resolveSibling("android-release.password")?.takeIf { it.isFile }?.readText()?.trim()

android {
    namespace = "dev.erfnemati.mirza"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.erfnemati.mirza"
        minSdk = 29
        targetSdk = 36
        versionCode = mirzaVersionCode
        versionName = mirzaVersion
    }

    signingConfigs {
        if (keystore != null && keystorePassword != null) {
            create("mirza") {
                storeFile = keystore
                storePassword = keystorePassword
                keyAlias = "mirza"
                keyPassword = keystorePassword
                storeType = "pkcs12"
            }
        } else {
            logger.warn("Mirza: release key not found; debug builds get the debug key, release builds stay unsigned")
        }
    }

    buildTypes {
        debug {
            signingConfigs.findByName("mirza")?.let { signingConfig = it }
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("mirza")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":core"))
    // 2026.08 and later need compileSdk 37.
    implementation(platform("androidx.compose:compose-bom:2026.06.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.activity:activity-compose:1.12.4")
}
