pluginManagement {
    val properties = java.util.Properties()
    val local = file("local.properties")
    if (local.exists()) local.inputStream().use(properties::load)
    val flutterSdk =
        properties.getProperty("flutter.sdk")
            ?: System.getenv("FLUTTER_ROOT")
            ?: error(
                "Set flutter.sdk in android/local.properties or FLUTTER_ROOT to the Flutter SDK"
            )
    settings.extra["flutterSdkPath"] = flutterSdk
    includeBuild("$flutterSdk/packages/flutter_tools/gradle")
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

plugins {
    id("org.jetbrains.kotlin.android") version "2.2.20" apply false
    id("com.android.application") version "9.1.0" apply false
    id("dev.flutter.flutter-plugin-loader") version "1.0.0"
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.PREFER_PROJECT)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "Resonate"

include(":app")
