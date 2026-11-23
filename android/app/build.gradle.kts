import com.android.build.api.variant.impl.VariantOutputImpl

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

// 릴리스 서명: 환경 변수(CI) 또는 같은 이름의 Gradle 속성(~/.gradle/gradle.properties).
// CASHBOOK_KEYSTORE_FILE이 없으면 릴리스 APK는 서명 없이 만들어진다.
fun signingValue(name: String): String? =
    providers.environmentVariable(name).orElse(providers.gradleProperty(name)).orNull
val releaseKeystore = signingValue("CASHBOOK_KEYSTORE_FILE")

android {
    namespace = "com.rouddy.cashbook.relay"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "com.rouddy.cashbook.relay"
        minSdk = 29
        targetSdk = 37
        versionCode = 1
        versionName = "0.2.0"
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = signingValue("CASHBOOK_KEYSTORE_PASSWORD")
                keyAlias = signingValue("CASHBOOK_KEY_ALIAS")
                keyPassword = signingValue("CASHBOOK_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            if (releaseKeystore != null) {
                signingConfig = signingConfigs.getByName("release")
            }
            optimization {
                enable = false
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
    }
}

// APK 이름: cashbook_<versionName>_<versionCode>.apk (빌드 타입은 폴더로 구분: outputs/apk/<debug|release>/)
// 새 Variant API에는 출력 파일 이름 설정이 공개되어 있지 않아 구현 클래스(VariantOutputImpl)를 쓴다.
androidComponents {
    onVariants { variant ->
        variant.outputs.forEach { output ->
            (output as? VariantOutputImpl)?.outputFileName?.set(
                output.versionName.zip(output.versionCode) { name, code -> "cashbook_${name}_$code.apk" },
            )
        }
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.androidx.datastore)
    implementation(libs.kotlinx.serialization.json)
    debugImplementation(libs.androidx.compose.ui.tooling)

    testImplementation(libs.junit)
}
