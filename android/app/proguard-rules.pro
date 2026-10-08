# Konstruado: reglas de R8 para el APK release.

# UniFFI: los bindings Kotlin usan JNA (Structure, Callback, Library) y JNA
# lee campos y métodos por reflexión; no se pueden renombrar ni quitar.
-keep class uniffi.** { *; }

# JNA
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.Structure { *; }
-keepclassmembers class * implements com.sun.jna.Callback { *; }
-dontwarn java.awt.**
-dontwarn com.sun.jna.**

# Métodos nativos en general.
-keepclasseswithmembernames class * {
    native <methods>;
}
