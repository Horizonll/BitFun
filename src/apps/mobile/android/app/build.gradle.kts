import java.io.File
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val mercuryAar: File? =
    file("libs")
        .listFiles()
        ?.filter { it.isFile && it.extension.equals("aar", ignoreCase = true) }
        ?.sortedByDescending { it.name }
        ?.firstOrNull()
val useVendorMercury: Boolean = mercuryAar != null

val localProperties = Properties().apply {
    val localFile = rootProject.file("local.properties")
    if (localFile.exists()) {
        localFile.inputStream().use { load(it) }
    }
}

fun localProp(name: String, default: String = ""): String =
    (localProperties.getProperty(name) ?: default)
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")

android {
    namespace = "com.bitfun.glasses"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.bitfun.glasses"
        minSdk = 26
        targetSdk = 34
        versionCode = 15
        versionName = "0.4.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        buildConfigField("boolean", "USE_VENDOR_MERCURY", useVendorMercury.toString())
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
        debug {
            applicationIdSuffix = ".debug"
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
        // Required by MercurySDK BaseMirrorActivity ViewBinding reflection.
        viewBinding = true
        buildConfig = true
    }

    testOptions {
        unitTests.isIncludeAndroidResources = false
    }
}

dependencies {
    if (useVendorMercury) {
        implementation(files(mercuryAar!!))
    } else {
        implementation(project(":mercury-stub"))
    }

    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("androidx.camera:camera-camera2:1.4.1")
    implementation("androidx.camera:camera-lifecycle:1.4.1")
    implementation("com.google.mlkit:barcode-scanning:17.3.0")

    // Optional on-device HiAI when the package exists on device.
    implementation("com.huawei.hiai.hiai-engine:huawei-hiai-asr:11.0.2.300")
    implementation("com.huawei.hiai.hiai-engine:huawei-hiai-pdk:11.0.2.300")

    testImplementation("junit:junit:4.13.2")
}
