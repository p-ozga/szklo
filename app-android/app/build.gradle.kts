import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "pl.szklo"
    compileSdk = 35

    defaultConfig {
        applicationId = "pl.szklo"
        minSdk = 26
        targetSdk = 35
        versionCode = 12
        versionName = "0.7.0"
        // Bez abiFilters: pakujemy to, co cargo-ndk położy w src/main/jniLibs
        // (domyślnie arm64-v8a i armeabi-v7a; opcjonalnie x86_64 dla emulatora – README).
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }

    // wspólne pliki z programem na PC (wizualizacja): <repo>/wspolne/wizualizacja → assets/wizualizacja
    sourceSets {
        getByName("main") {
            assets.srcDir("../../wspolne")
        }
    }

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.kotlinx.coroutines.android)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.foundation)
    implementation(libs.androidx.compose.animation)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui.tooling.preview)
    debugImplementation(libs.androidx.compose.ui.tooling)

    testImplementation(libs.junit)
}

// ---------------------------------------------------------------------------------------------
// Rdzeń DSP (Rust) → src/main/jniLibs/<abi>/libszklo_jni.so przez cargo-ndk.
// Ręcznie:      ./gradlew :app:cargoNdkBuild
// Przy buildzie: w gradle.properties ustaw szklo.buildRust=true (albo -Pszklo.buildRust=true).
// ---------------------------------------------------------------------------------------------
val rustDir = rootProject.layout.projectDirectory.dir("rust")
val jniLibsDir = layout.projectDirectory.dir("src/main/jniLibs")

val cargoNdkBuild = tasks.register<Exec>("cargoNdkBuild") {
    group = "rust"
    description = "Buduje libszklo_jni.so (arm64-v8a, armeabi-v7a) przez cargo-ndk."
    workingDir = rustDir.asFile
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a",
        "-t", "armeabi-v7a",
        "-P", "26", // AAudio (cpal) wymaga API 26
        "-o", jniLibsDir.asFile.absolutePath,
        "build", "--release",
    )
}

if (providers.gradleProperty("szklo.buildRust").orNull == "true") {
    tasks.named("preBuild") { dependsOn(cargoNdkBuild) }
}
