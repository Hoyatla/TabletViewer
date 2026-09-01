# Add project-specific ProGuard rules here.
# By default, the flags in this file are appended to flags specified in
# proguard-android-optimize.txt. You can edit the corresponding file to
# change the default settings.
#
# For more details, see https://developer.android.com/studio/build/shrink-code

# Keep all classes in the screenviewer package — we use reflection-free
# direct method calls and want full symbol info in stack traces.
-keep class com.nextos.screenviewer.** { *; }
