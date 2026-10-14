plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.resonate.android"
    compileSdk = 36
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "org.resonate.android"
        minSdk = 36
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    buildFeatures { compose = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging { jniLibs.useLegacyPackaging = false }
}

abstract class RustBuild : DefaultTask() {
    @get:OutputDirectory abstract val kotlinDirectory: DirectoryProperty
    @get:OutputDirectory abstract val nativeDirectory: DirectoryProperty
    @get:Internal abstract val sdkDirectory: DirectoryProperty
    @get:Internal abstract val repository: DirectoryProperty
    @get:Inject abstract val execution: ExecOperations

    @TaskAction
    fun build() {
        execution.exec {
            workingDir(repository.get().asFile)
            commandLine(
                if (System.getProperty("os.name").startsWith("Windows")) "python" else "python3",
                repository.file("android/build-native.py").get().asFile,
                "--sdk",
                sdkDirectory.get().asFile.absolutePath,
            )
        }
    }
}

val buildRust by
    tasks.registering(RustBuild::class) {
        repository.set(rootProject.layout.projectDirectory.dir(".."))
        sdkDirectory.set(androidComponents.sdkComponents.sdkDirectory)
        inputs.files(
            fileTree(repository.dir("crates")) {
                include("**/*.rs", "**/Cargo.toml", "**/uniffi.toml")
            }
        )
        inputs.files(
            repository.file("Cargo.toml"),
            repository.file("Cargo.lock"),
            repository.file("android/build-native.py"),
        )
        kotlinDirectory.set(layout.buildDirectory.dir("generated/uniffi"))
        nativeDirectory.set(layout.buildDirectory.dir("generated/jniLibs"))
    }

androidComponents.onVariants { variant ->
    variant.sources.java?.addGeneratedSourceDirectory(buildRust) { it.kotlinDirectory }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(buildRust) { it.nativeDirectory }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.03.00"))
    implementation("androidx.activity:activity-compose:1.12.4")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.media3:media3-session:1.9.3")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
}
