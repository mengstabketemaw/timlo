# Android build environment for timlo (SDK and JDK installed under $HOME).
# Sourced by the app's android npm scripts; source it yourself to run
# `npx tauri android …` or gradle directly. Paths can be overridden.
export JAVA_HOME="${TIMLO_JAVA_HOME:-$HOME/.local/share/jdk/jdk-21.0.13+9}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export NDK_HOME="${NDK_HOME:-$ANDROID_HOME/ndk/27.2.12479018}"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
