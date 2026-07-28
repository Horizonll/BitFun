import java.io.File

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val mercuryAar: File = file("libs/mercury-sdk.aar")
val useVendorMercury: Boolean = mercuryAar.exists()

android {
    namespace = "com.bitfun.glasses"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.bitfun.glasses"
        minSdk = 26
        targetSdk = 34
        versionCode = 14
        versionName = "0.3.4"
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
        viewBinding = false
        buildConfig = true
    }

    testOptions {
        unitTests.isIncludeAndroidResources = false
    }
}

dependencies {
    if (useVendorMercury) {
        implementation(files(mercuryAar))
    } else {
        implementation(project(":mercury-stub"))
    }

    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.camera:camera-camera2:1.4.1")
    implementation("androidx.camera:camera-lifecycle:1.4.1")
    implementation("androidx.camera:camera-view:1.4.1")
    implementation("com.google.mlkit:barcode-scanning:17.3.0")

    // On-device ASR for Huawei/RayNeo (device ships com.huawei.hiai asr.apk).
    implementation("com.huawei.hiai.hiai-engine:huawei-hiai-asr:11.0.2.300")
    implementation("com.huawei.hiai.hiai-engine:huawei-hiai-pdk:11.0.2.300")

    testImplementation("junit:junit:4.13.2")
}
