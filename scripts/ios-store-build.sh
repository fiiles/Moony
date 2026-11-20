#!/bin/bash
# Builds the iPhone app for App Store Connect (TestFlight and the App Store) and checks the result.
# It does not upload anything: the last lines say how. The whole procedure, the one-time setup and
# what the checks mean are in docs/playbooks/release-ios.md.
#
#   scripts/ios-store-build.sh                     signed .ipa for App Store Connect
#   scripts/ios-store-build.sh --dry-run           unsigned .ipa, to check the bundle without an account
#   scripts/ios-store-build.sh --build-number 42   a build number of your own (digits only)
#
# The device-install recipe (`tauri ios build --debug --export-method debugging`) is untouched: this
# script passes its export method and its build number on the command line and puts the two tracked
# files the Tauri CLI rewrites during a build (Info.plist, project.pbxproj) back when it ends.
set -euo pipefail

usage() {
  sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
}

dry_run=false
build_number="${MOONY_IOS_BUILD_NUMBER:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry_run=true ;;
    --build-number)
      [ $# -ge 2 ] || { usage >&2; exit 2; }
      build_number="$2"
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
  shift
done

cd "$(dirname "$0")/.."

apple="src-tauri/gen/apple"
info_plist="$apple/moony_iOS/Info.plist"
pbxproj="$apple/moony.xcodeproj/project.pbxproj"
ipa="$apple/build/arm64/Moony.ipa"
identifier="com.moonyfinance.mobile"

fail() {
  echo "ios-store-build: $*" >&2
  exit 1
}

# The build number (CFBundleVersion): App Store Connect takes each number once per version, and a
# later upload needs a higher one. The default is the UTC minute of the build (YYMMDDHHMM), which
# rises by itself and needs nothing remembered or committed.
[ -n "$build_number" ] || build_number="$(date -u +%y%m%d%H%M)"
case "$build_number" in
  *[!0-9]* | 0* | "") fail "the build number must be a whole number without a leading zero, got '$build_number'" ;;
esac
[ "${#build_number}" -le 18 ] || fail "the build number is longer than the 18 characters App Store Connect takes"

version="$(node -p "require('./package.json').version")"

# The Tauri CLI writes the version, the build number and the signing team into these two tracked
# files at every build. Keep the checkout as it was.
backup="$(mktemp -d)"
cp "$info_plist" "$backup/Info.plist"
cp "$pbxproj" "$backup/project.pbxproj"
unpacked=""
restore() {
  cp "$backup/Info.plist" "$info_plist"
  cp "$backup/project.pbxproj" "$pbxproj"
  rm -rf "$backup"
  [ -z "$unpacked" ] || rm -rf "$unpacked"
}
trap restore EXIT

rm -f "$ipa"

args=(ios build --ci --config "{\"bundle\":{\"iOS\":{\"bundleVersion\":\"$build_number\"}}}")
if $dry_run; then
  args+=(--no-sign)
else
  args+=(--export-method app-store-connect)
fi

echo "ios-store-build: Moony $version ($build_number)$($dry_run && echo ', unsigned dry run' || true)"
npm run tauri -- "${args[@]}"

[ -f "$ipa" ] || fail "the build left no $ipa"

# What is in the package, read back from it
unpacked="$(mktemp -d)"
unzip -q "$ipa" -d "$unpacked"
app="$unpacked/Payload/Moony.app"
[ -d "$app" ] || fail "no Payload/Moony.app in $ipa"
read_plist() {
  /usr/libexec/PlistBuddy -c "Print :$1" "$app/Info.plist" 2>/dev/null || true
}
expect() {
  [ "$(read_plist "$1")" = "$2" ] || fail "$1 in the package is '$(read_plist "$1")', expected '$2'"
}
expect CFBundleIdentifier "$identifier"
expect CFBundleShortVersionString "$version"
expect CFBundleVersion "$build_number"
[ -f "$app/PrivacyInfo.xcprivacy" ] || fail "PrivacyInfo.xcprivacy is missing from the package"
plutil -lint "$app/PrivacyInfo.xcprivacy" >/dev/null || fail "PrivacyInfo.xcprivacy in the package is not a valid property list"

echo
echo "ios-store-build: $ipa"
echo "  Moony $version ($build_number), $identifier, privacy manifest present"
if $dry_run; then
  echo "  Unsigned dry run: this package cannot be uploaded."
else
  echo "  Upload it yourself (docs/playbooks/release-ios.md, \"Upload\"), for example:"
  echo "    xcrun altool --validate-app \"$ipa\" --api-key \"\$APPLE_API_KEY\" --api-issuer \"\$APPLE_API_ISSUER\""
  echo "    xcrun altool --upload-package \"$ipa\" --api-key \"\$APPLE_API_KEY\" --api-issuer \"\$APPLE_API_ISSUER\""
fi
