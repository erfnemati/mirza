# JNA finds the bindings' classes, fields and callbacks by name at run time.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class dev.erfnemati.mirza.core.** { *; }
-dontwarn java.awt.**
