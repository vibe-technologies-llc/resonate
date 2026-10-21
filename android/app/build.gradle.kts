plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

fun signingValue(property: String, environment: String): String? =
    (findProperty(property) as String?)?.takeIf { it.isNotBlank() }
        ?: System.getenv(environment)?.takeIf { it.isNotBlank() }

val releaseKeystore = signingValue("resonate.signing.storeFile", "RESONATE_SIGNING_STORE_FILE")

android {
    namespace = "org.resonate.android"
    compileSdk = 37
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "org.resonate.android"
        minSdk = 33
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword =
                    requireNotNull(
                        signingValue(
                            "resonate.signing.storePassword",
                            "RESONATE_SIGNING_STORE_PASSWORD",
                        )
                    ) {
                        "Set resonate.signing.storePassword or RESONATE_SIGNING_STORE_PASSWORD"
                    }
                keyAlias =
                    requireNotNull(
                        signingValue("resonate.signing.keyAlias", "RESONATE_SIGNING_KEY_ALIAS")
                    ) {
                        "Set resonate.signing.keyAlias or RESONATE_SIGNING_KEY_ALIAS"
                    }
                keyPassword =
                    requireNotNull(
                        signingValue(
                            "resonate.signing.keyPassword",
                            "RESONATE_SIGNING_KEY_PASSWORD",
                        )
                    ) {
                        "Set resonate.signing.keyPassword or RESONATE_SIGNING_KEY_PASSWORD"
                    }
            }
        }
    }
    buildTypes {
        getByName("debug") {
            ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        }
        getByName("release") {
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            ndk { abiFilters += listOf("arm64-v8a") }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures { compose = true }
    packaging { jniLibs.useLegacyPackaging = false }
}

kotlin {
    compilerOptions {
        optIn.addAll(
            "androidx.compose.material3.ExperimentalMaterial3Api",
            "androidx.compose.material3.ExperimentalMaterial3ExpressiveApi",
            "androidx.compose.foundation.ExperimentalFoundationApi",
        )
    }
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

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.material3:material3:1.5.0-beta01")
    implementation("androidx.compose.material3:material3-adaptive-navigation-suite:1.5.0-beta01")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.11.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-navigation3:2.11.0")
    implementation("androidx.navigation3:navigation3-runtime:1.2.0")
    implementation("androidx.navigation3:navigation3-ui:1.2.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-core:1.12.0")
    implementation("com.materialkolor:material-kolor:5.0.1")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.media3:media3-session:1.9.3")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
    androidTestImplementation(platform("androidx.compose:compose-bom:2026.09.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
}
