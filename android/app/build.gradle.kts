plugins {
    id("com.android.application")
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "org.resonate.android"
    compileSdk = 36
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "org.resonate.android"
        minSdk = 33
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    buildTypes {
        getByName("debug") {
            ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        }
        getByName("release") {
            signingConfig = signingConfigs.getByName("debug")
            ndk { abiFilters += listOf("arm64-v8a") }
        }
    }
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
    @get:Input abstract val profile: Property<String>
    @get:Input abstract val abis: ListProperty<String>
    @get:Inject abstract val execution: ExecOperations

    @TaskAction
    fun build() {
        val requestedAbis = abis.get()
        execution.exec {
            workingDir(repository.get().asFile)
            val command =
                mutableListOf(
                    if (System.getProperty("os.name").startsWith("Windows")) "python"
                    else "python3",
                    repository.file("android/build-native.py").get().asFile.absolutePath,
                    "--sdk",
                    sdkDirectory.get().asFile.absolutePath,
                    "--profile",
                    profile.get(),
                    "--output",
                    kotlinDirectory.get().asFile.parentFile.absolutePath,
                    "--abis",
                )
            command.addAll(requestedAbis)
            commandLine(command)
        }
    }
}

androidComponents.onVariants { variant ->
    val release = variant.buildType == "release"
    val generated = layout.buildDirectory.dir(if (release) "generated/release" else "generated")
    val buildRust =
        tasks.register<RustBuild>("buildRust${variant.name.replaceFirstChar { it.uppercase() }}") {
            repository.set(rootProject.layout.projectDirectory.dir(".."))
            sdkDirectory.set(androidComponents.sdkComponents.sdkDirectory)
            profile.set(if (release) "release" else "debug")
            abis.set(if (release) listOf("arm64-v8a") else listOf("arm64-v8a", "x86_64"))
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
            kotlinDirectory.set(generated.map { it.dir("uniffi") })
            nativeDirectory.set(generated.map { it.dir("jniLibs") })
        }
    variant.sources.java?.addGeneratedSourceDirectory(buildRust) { it.kotlinDirectory }
    variant.sources.jniLibs?.addGeneratedSourceDirectory(buildRust) { it.nativeDirectory }
}

flutter { source = "../flutter" }

dependencies {
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.media3:media3-session:1.9.3")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
}
