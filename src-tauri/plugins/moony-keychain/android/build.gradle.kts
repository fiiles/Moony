import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.moonyfinance.keychain"
    compileSdk = 36

    defaultConfig {
        // setInvalidatedByBiometricEnrollment needs API 24.
        minSdk = 24
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    // The versions tauri-plugin-biometric 2 used when this was written (the app no longer
    // bundles that plugin; the in-repo keychain replaced it).
    implementation("androidx.biometric:biometric:1.1.0")
    implementation("androidx.core:core-ktx:1.9.0")
    implementation("androidx.appcompat:appcompat:1.6.0")
    implementation(project(":tauri-android"))
}
