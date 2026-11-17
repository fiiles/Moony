# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile
# rustls-platform-verifier's Kotlin half is called only through JNI (reqwest's TLS on Android):
# keep it from being stripped as dead code.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }

# ML Kit (the barcode scanner plugin's QR reader for device pairing) finds its components by
# reflection: Firebase's ComponentDiscovery instantiates each registrar named in the manifest
# through its no-argument constructor. R8 full mode (AGP 9) removed those constructors in the
# release build ("NoSuchMethodException: ...BarcodeRegistrar.<init>" at start), so the scanner
# would have no barcode component. Keep every registrar with its constructor.
-keep class * implements com.google.firebase.components.ComponentRegistrar { <init>(); }
