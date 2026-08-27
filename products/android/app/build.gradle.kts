plugins {
    id("com.android.application")
}

fun String.asBuildConfigString(): String =
    "\"${replace("\\", "\\\\").replace("\"", "\\\"")}\""

val telaBundleIndex = providers.gradleProperty("telaBundleIndex").orElse("").get()
val telaAppId = providers.gradleProperty("telaAppId").orElse("dev.tela.mobile").get()
val telaRelayUrl = providers.gradleProperty("telaRelayUrl").orElse("").get()
val telaRelayToken = providers.gradleProperty("telaRelayToken").orElse("").get()

android {
    namespace = "dev.tela.mobile"
    compileSdk = 36
    ndkVersion = "27.1.12297006"

    defaultConfig {
        applicationId = telaAppId
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-dev"
        ndk {
            abiFilters += "arm64-v8a"
        }
        buildConfigField("String", "TELA_BUNDLE_INDEX", telaBundleIndex.asBuildConfigString())
        // CC Remote 中继配置（可选）：注入后 Rust 宿主在 android_main 前建立 net 桥。
        buildConfigField("String", "TELA_RELAY_URL", telaRelayUrl.asBuildConfigString())
        buildConfigField("String", "TELA_RELAY_TOKEN", telaRelayToken.asBuildConfigString())
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".dev"
            versionNameSuffix = "-debug"
        }
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        buildConfig = true
    }

    sourceSets {
        getByName("main").jniLibs.srcDir("src/main/jniLibs")
    }

    packaging {
        jniLibs {
            useLegacyPackaging = false
            // The WSL host links with Android Studio's Windows NDK. Keep symbols in this
            // development APK instead of asking AGP to run a nonexistent Linux llvm-strip.
            keepDebugSymbols += "**/*.so"
        }
    }
}

dependencies {
    // games-activity publishes its GameActivity bytecode without its AndroidX base classes.
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.games:games-activity:4.4.0")
}
