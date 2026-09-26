plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.tirodz.emergencysimulator"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.tirodz.emergencysimulator"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "1.0.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

dependencies {
    // Plain JVM unit tests. `AlertRequest` is deliberately free of Android framework types so the
    // validation and the log-injection guard can be tested without an emulator or Robolectric.
    testImplementation("junit:junit:4.13.2")
}
