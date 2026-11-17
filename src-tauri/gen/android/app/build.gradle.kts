import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

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

// Release signing with the upload key (Play App Signing re-signs for the store). Read from the
// MOONY_ANDROID_* environment variables (CI, .github/workflows/android-bundle.yml) or from
// gen/android/keystore.properties (local, git-ignored): storeFile, storePassword, keyAlias,
// keyPassword. Without them a release build is unsigned and Play refuses it
// (docs/playbooks/release.md, "Android: upload key and Play bundle").
val keystoreProperties = Properties().apply {
    val propFile = rootProject.file("keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}
fun signingValue(property: String, variable: String): String? =
    System.getenv(variable)?.takeIf { it.isNotEmpty() } ?: keystoreProperties.getProperty(property)
val releaseStoreFile = signingValue("storeFile", "MOONY_ANDROID_KEYSTORE_PATH")

// The Kotlin half of rustls-platform-verifier, which reqwest's TLS calls through JNI to check
// certificates with Android's trust manager (initialised in lib.rs, `init_android_tls_verifier`).
// The rustls project publishes it as a Maven repository on GitHub (branch `maven-archive`); its
// version must be the one of the `rustls-platform-verifier-android` crate in Cargo.lock, so it is
// read from there. The repository is pinned to a commit of that branch (content-addressed, so
// what is downloaded cannot change under the same URL) and may serve only the org.rustls group.
// When Cargo.lock moves to a version this commit lacks, the build fails: point it at the newer
// commit (`gh api repos/rustls/rustls-platform-verifier/branches/maven-archive --jq .commit.sha`).
repositories {
    maven {
        url = uri("https://raw.githubusercontent.com/rustls/rustls-platform-verifier/1aa691352a5e0c215210dfc9a3c2f2078639dc91/android-release-support/maven/")
        content { includeGroup("org.rustls") }
    }
}

val rustlsPlatformVerifierVersion: String = file("../../../Cargo.lock").readLines().let { lines ->
    val crate = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
    check(crate >= 0) { "rustls-platform-verifier-android not found in Cargo.lock" }
    lines.drop(crate + 1).first { it.trimStart().startsWith("version = ") }
        .substringAfter('"').substringBefore('"')
}

android {
    compileSdk = 37
    namespace = "com.moonyfinance.mobile"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.moonyfinance.mobile"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (releaseStoreFile != null) {
            create("release") {
                storeFile = rootProject.file(releaseStoreFile)
                storePassword = signingValue("storePassword", "MOONY_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = signingValue("keyAlias", "MOONY_ANDROID_KEY_ALIAS")
                keyPassword = signingValue("keyPassword", "MOONY_ANDROID_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfigs.findByName("release")?.let { signingConfig = it }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    implementation("org.rustls:rustls-platform-verifier:$rustlsPlatformVerifierVersion")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
