pluginManagement {
    val flutterSdkPath = run {
        val properties = java.util.Properties()
        file("local.properties").inputStream().use { properties.load(it) }
        val flutterSdkPath = properties.getProperty("flutter.sdk")
        require(flutterSdkPath != null) { "flutter.sdk not set in local.properties" }
        flutterSdkPath
    }

    includeBuild("$flutterSdkPath/packages/flutter_tools/gradle")

    repositories {
        maven {
            setUrl("https://maven.aliyun.com/repository/gradle-plugin")
        }
        google()
        maven {
            setUrl("https://maven.aliyun.com/repository/public")
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

plugins {
    id("dev.flutter.flutter-plugin-loader") version "1.0.0"
    id("com.android.application") version "8.11.1" apply false
    id("org.jetbrains.kotlin.android") version "2.2.20" apply false
}

// NOTE: no dependencyResolutionManagement here. Flutter's gradle plugin adds
// the `download.flutter.io` engine Maven repo at configure time; a
// PREFER_SETTINGS mode would block that repo and break engine artifact
// resolution (io.flutter:*_debug).

include(":app")
