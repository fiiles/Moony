#!/bin/sh
# Environment for `tauri android` (sourced by the android:* npm scripts and the CI job).
#
# Defaults match a macOS machine with Android Studio and its SDK in the usual places; anything
# already set (CI, another SDK location) wins. Also points the cc crate at the NDK's llvm-ranlib:
# NDK r23+ ships no `<triple>-ranlib`, and the vendored OpenSSL that SQLCipher links on Android
# (libsqlite3-sys `bundled-sqlcipher-vendored-openssl`) fails its install step without one.

if [ -z "$JAVA_HOME" ] && [ -d "/Applications/Android Studio.app/Contents/jbr/Contents/Home" ]; then
  export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
fi

if [ -z "$ANDROID_HOME" ]; then
  if [ -n "$ANDROID_SDK_ROOT" ]; then
    export ANDROID_HOME="$ANDROID_SDK_ROOT"
  elif [ -d "$HOME/Library/Android/sdk" ]; then
    export ANDROID_HOME="$HOME/Library/Android/sdk"
  fi
fi

if [ -z "$NDK_HOME" ] && [ -d "$ANDROID_HOME/ndk" ]; then
  # The newest NDK installed side by side
  NDK_HOME="$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -n 1)"
  export NDK_HOME
fi

case "$(uname -s)" in
  Darwin) ndk_host="darwin-x86_64" ;;
  *) ndk_host="linux-x86_64" ;;
esac
llvm_ranlib="$NDK_HOME/toolchains/llvm/prebuilt/$ndk_host/bin/llvm-ranlib"
if [ -x "$llvm_ranlib" ]; then
  export RANLIB_aarch64_linux_android="$llvm_ranlib"
  export RANLIB_armv7_linux_androideabi="$llvm_ranlib"
  export RANLIB_i686_linux_android="$llvm_ranlib"
  export RANLIB_x86_64_linux_android="$llvm_ranlib"
fi
