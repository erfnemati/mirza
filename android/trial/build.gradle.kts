plugins {
    id("com.android.application")
}

android {
    namespace = "dev.erfnemati.mirza"
    compileSdk = 36

    defaultConfig {
        // The trial gets its own id, so it can sit next to the real app later.
        applicationId = "dev.erfnemati.mirza.trial"
        minSdk = 29
        targetSdk = 36
        versionCode = 3
        versionName = "0.1.5-trial3"
    }

    buildFeatures { buildConfig = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":core"))
}
