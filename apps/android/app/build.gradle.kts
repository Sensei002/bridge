plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}
android {
    namespace = "app.bridge.android"
    compileSdk = 36
    defaultConfig {
        applicationId = "app.bridge.android"
        minSdk = 26
        targetSdk = 36
        versionCode = providers.gradleProperty("bridgeVersionCode").orElse("2").get().toInt()
        versionName = providers.gradleProperty("bridgeVersionName").orElse("0.2.0").get()
    }
    signingConfigs {
        create("release") {
            val storePath = System.getenv("BRIDGE_KEYSTORE_PATH")
            if (!storePath.isNullOrBlank()) {
                storeFile = file(storePath)
                storePassword = System.getenv("BRIDGE_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("BRIDGE_KEY_ALIAS")
                keyPassword = System.getenv("BRIDGE_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            if (!System.getenv("BRIDGE_KEYSTORE_PATH").isNullOrBlank()) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
}
dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.01.00")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.10.0")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
