plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "com.astral.aml"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }

    kotlinOptions {
        jvmTarget = JavaVersion.VERSION_11.toString()
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "com.astral.aml"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
        // Restrict native ABIs: arm64 (real devices) + x86_64 (emulator).
        // Avoids pulling unused Flutter engine ABIs (e.g. armeabi_v7a).
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            // TODO: Add your own signing config for the release build.
            // Signing with the debug keys for now, so `flutter run --release` works.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    packaging {
        jniLibs {
            // The JRE `.so` set must exist as real files on disk so the runtime
            // loader can raw-`dlopen` them (libjsig/libjli/libjvm in order) and
            // the HotSpot VM can open libjava/libzip/etc. AGP's default
            // (useLegacyPackaging=false) only mmaps libs from inside the APK,
            // leaving `nativeLibraryDir` empty — dlopen would find nothing.
            useLegacyPackaging = true
        }
    }
}

flutter {
    source = "../.."
}
