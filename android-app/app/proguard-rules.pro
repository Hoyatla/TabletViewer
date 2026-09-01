# Add project-specific ProGuard rules here.
# By default, the flags in this file are appended to flags specified in
# proguard-android-optimize.txt. You can edit the corresponding file to
# change the default settings.
#
# For more details, see https://developer.android.com/studio/build/shrink-code

# Keep all classes in the screenviewer package — we use reflection-free
# direct method calls and want full symbol info in stack traces.
-keep class com.nextos.screenviewer.** { *; }

# OkHttp 4.x doesn't need any keep rules; the consumer rules are bundled
# in the AAR/JAR. But keep its internal classes for reflection-heavy
# extensions we don't use here.

# kotlinx-coroutines: no keep rules needed for our usage.

# org.json is part of the Android framework (API 26+), so we don't need
# any rules for it either.
