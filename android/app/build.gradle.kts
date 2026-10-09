plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// App version comes from the Rust workspace (`[workspace.package] version`),
// the same value the desktop window and konstruado-ffi report.
val konstruadoVersion: String = run {
    val toml = rootProject.file("../Cargo.toml").readText()
    val section = toml.substringAfter("[workspace.package]").substringBefore("\n[")
    Regex("""(?m)^version\s*=\s*"([^"]+)"""").find(section)?.groupValues?.get(1)
        ?: error("version not found in [workspace.package] of Cargo.toml")
}
val konstruadoVersionCode: Int = konstruadoVersion.substringBefore('-').split('.')
    .map { it.toInt() }
    .let { (major, minor, patch) -> major * 10000 + minor * 100 + patch }

// Firma release: de variables de entorno o propiedades de Gradle
// (KONSTRUADO_RELEASE_STORE_FILE, _STORE_PASSWORD, _KEY_ALIAS, _KEY_PASSWORD).
// El keystore vive fuera del repo; sin esos datos el release se firma con la
// clave debug para que cualquiera pueda compilarlo.
fun firma(nombre: String): String? =
    (project.findProperty(nombre) as String?) ?: System.getenv(nombre)
val releaseStore: String? = firma("KONSTRUADO_RELEASE_STORE_FILE")?.takeIf { file(it).exists() }

// ABIs: por defecto arm64 + x86_64 (emulador). El script de release pasa -Pabis=arm64-v8a.
val abis: List<String> = (project.findProperty("abis") as String?)
    ?.split(',')?.map { it.trim() }?.filter { it.isNotEmpty() }
    ?: listOf("arm64-v8a", "x86_64")

android {
    namespace = "cl.konstruado.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "cl.konstruado.app"
        minSdk = 26
        targetSdk = 34
        versionCode = konstruadoVersionCode
        versionName = konstruadoVersion
        ndk {
            abiFilters += abis
        }
    }

    signingConfigs {
        if (releaseStore != null) {
            create("release") {
                storeFile = file(releaseStore)
                storePassword = firma("KONSTRUADO_RELEASE_STORE_PASSWORD")
                keyAlias = firma("KONSTRUADO_RELEASE_KEY_ALIAS")
                keyPassword = firma("KONSTRUADO_RELEASE_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            // R8: reduce y ofusca Kotlin/Compose; las clases de UniFFI y JNA se
            // mantienen (proguard-rules.pro) porque JNA las busca por reflexión.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            signingConfig = signingConfigs.findByName("release")
                ?: signingConfigs.getByName("debug")
        }
        debug {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }
    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
        jniLibs {
            useLegacyPackaging = true
        }
    }
    testOptions {
        unitTests.isIncludeAndroidResources = true
        unitTests.all {
            // Capturas Compose (Robolectric + Roborazzi): solo con -Pcapturas=DIR.
            (project.findProperty("capturas") as String?)?.let { dir ->
                it.systemProperty("konstruado.capturas", dir)
                it.systemProperty("roborazzi.test.record", "true")
            }
            (project.findProperty("demo") as String?)?.let { d -> it.systemProperty("konstruado.demo", d) }
            (project.findProperty("oscuro") as String?)?.let { o -> it.systemProperty("konstruado.oscuro", o) }
            (project.findProperty("idioma") as String?)?.let { i -> it.systemProperty("konstruado.idioma", i) }
            it.systemProperty("jna.library.path", rootProject.file("../target/debug").absolutePath)
            it.testLogging { showStandardStreams = true; events("passed", "failed") }
            // Las capturas usan ui-test-manifest (solo debug); en release corren los tests del motor.
            if (it.name.contains("Release")) it.filter.excludeTestsMatching("cl.konstruado.app.capturas.*")
        }
    }
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.06.00")
    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.3")
    implementation("androidx.navigation:navigation-compose:2.7.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    debugImplementation("androidx.compose.ui:ui-tooling")
    // UniFFI Kotlin bindings
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    // Test JVM: bindings + JNA de escritorio + lib del host (target/debug).
    testImplementation("junit:junit:4.13.2")
    testImplementation("net.java.dev.jna:jna:5.14.0")
    // Capturas de pantalla en la JVM (sin emulador).
    testImplementation("org.robolectric:robolectric:4.12.2")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.20.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.20.0")
    testImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}

