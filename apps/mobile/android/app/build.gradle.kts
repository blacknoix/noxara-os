import java.util.Properties
import java.io.FileInputStream

plugins {
    id("com.android.application")
    id("kotlin-android")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "com.companyos.companyos_mobile"
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
        applicationId = "com.companyos.companyos_mobile"
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    // Signing: CI sets COMPANYOS_UPLOAD_* env vars (ephemeral CI-only keystore).
    // Optional local override via android/key.properties (gitignored).
    // Play Console upload key for production must come from GitHub secrets —
    // never commit a production keystore. See docs/clients/store-release.md.
    val keystorePropertiesFile = rootProject.file("key.properties")
    val keystoreProperties = Properties()
    if (keystorePropertiesFile.exists()) {
        keystoreProperties.load(FileInputStream(keystorePropertiesFile))
    }

    fun envOrProp(envKey: String, propKey: String): String? {
        val fromEnv = System.getenv(envKey)
        if (!fromEnv.isNullOrBlank()) return fromEnv
        val fromProp = keystoreProperties.getProperty(propKey)
        if (!fromProp.isNullOrBlank()) return fromProp
        return null
    }

    val uploadStoreFile = envOrProp("COMPANYOS_UPLOAD_STORE_FILE", "storeFile")
    val uploadStorePassword = envOrProp("COMPANYOS_UPLOAD_STORE_PASSWORD", "storePassword")
    val uploadKeyAlias = envOrProp("COMPANYOS_UPLOAD_KEY_ALIAS", "keyAlias")
    val uploadKeyPassword = envOrProp("COMPANYOS_UPLOAD_KEY_PASSWORD", "keyPassword")
    val hasUploadSigning =
        !uploadStoreFile.isNullOrBlank() &&
            !uploadStorePassword.isNullOrBlank() &&
            !uploadKeyAlias.isNullOrBlank() &&
            !uploadKeyPassword.isNullOrBlank()

    signingConfigs {
        create("upload") {
            if (hasUploadSigning) {
                storeFile = file(uploadStoreFile!!)
                storePassword = uploadStorePassword
                keyAlias = uploadKeyAlias
                keyPassword = uploadKeyPassword
            }
        }
    }

    buildTypes {
        release {
            if (hasUploadSigning) {
                signingConfig = signingConfigs.getByName("upload")
            } else {
                // Local `flutter run --release` only. CI android-signed-release
                // must set COMPANYOS_UPLOAD_* so the artifact is upload-signed.
                signingConfig = signingConfigs.getByName("debug")
            }
        }
    }
}

flutter {
    source = "../.."
}
