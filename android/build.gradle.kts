plugins {
    id("com.android.application") version "9.5.1" apply false
    id("com.android.library") version "9.5.1" apply false
    // The Compose compiler matches the Kotlin that AGP 9 has built in.
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10" apply false
}
