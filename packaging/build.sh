#!/usr/bin/env bash
# Build an installer for one platform.
#
# One script rather than three blocks of YAML, because packaging is the part of
# a build somebody has to be able to run *locally* when it goes wrong — and a
# step that only exists inside a CI workflow can only be debugged by pushing.
#
#   packaging/build.sh msi | dmg | appimage
#
# Everything lands in packaging/out.
set -euo pipefail

kind=${1:?"say which: msi, dmg or appimage"}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/packaging/out"
binary="$root/target/release/tessera_app"
[ "$kind" = "msi" ] && binary="$binary.exe"

version=$(grep -m1 '^version' "$root/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
name="Tessera Publisher"
slug="tessera-publisher"

rm -rf "$out"
mkdir -p "$out"

if [ ! -f "$binary" ]; then
  echo "No binary at $binary. Run: cargo build --release -p tessera_app" >&2
  exit 1
fi

# The profiles travel with the application, beside it, which is the first place
# `bundled_directory` looks. A release that left them behind would start,
# and quietly be unable to soft-proof or export PDF/X.
stage_shared() {
  local into=$1
  mkdir -p "$into/profiles"
  cp "$root"/assets/profiles/*.icc "$into/profiles/" 2>/dev/null || true
  cp "$root/assets/profiles/manifest.tsv" "$into/profiles/"
  cp "$root/assets/profiles/LICENCES.md" "$into/profiles/" 2>/dev/null || true
}

case "$kind" in
  appimage)
    app="$out/$slug.AppDir"
    mkdir -p "$app/usr/bin" "$app/usr/share/applications" \
             "$app/usr/share/icons/hicolor/512x512/apps"
    cp "$binary" "$app/usr/bin/$slug"
    stage_shared "$app/usr/bin"
    cp "$root/packaging/linux/$slug.desktop" "$app/usr/share/applications/"
    cp "$root/packaging/linux/$slug.desktop" "$app/"
    cp "$root/assets/tessera-publisher-logotype.png" \
       "$app/usr/share/icons/hicolor/512x512/apps/$slug.png"
    cp "$root/assets/tessera-publisher-logotype.png" "$app/$slug.png"
    printf '#!/bin/sh\nexec "$(dirname "$0")/usr/bin/%s" "$@"\n' "$slug" > "$app/AppRun"
    chmod +x "$app/AppRun"

    # Checked before it is packaged: a malformed .desktop file installs without
    # complaint and then the application simply is not in the menu, with
    # nothing anywhere saying why.
    desktop-file-validate "$app/usr/share/applications/$slug.desktop"

    if command -v appimagetool >/dev/null 2>&1; then
      appimagetool "$app" "$out/$name-$version-x86_64.AppImage"
      rm -rf "$app"
    else
      echo "appimagetool not found: leaving the AppDir for a machine that has it"
    fi
    ;;

  dmg)
    app="$out/$name.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$binary" "$app/Contents/MacOS/$slug"
    stage_shared "$app/Contents/Resources"
    sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" \
      > "$app/Contents/Info.plist"
    cp "$root/assets/tessera-publisher-logotype.png" \
       "$app/Contents/Resources/$slug.png"

    hdiutil create -volname "$name" -srcfolder "$app" -ov -format UDZO \
      "$out/$name-$version.dmg"
    rm -rf "$app"
    ;;

  msi)
    # cargo-wix drives the WiX toolset from packaging/windows/main.wxs.
    if ! command -v cargo-wix >/dev/null 2>&1; then
      cargo install cargo-wix --locked
    fi
    cargo wix --package tessera_app --nocapture \
      --output "$out/$name-$version-x86_64.msi"
    ;;

  *)
    echo "unknown kind: $kind" >&2
    exit 1
    ;;
esac

echo "Built:"
ls -la "$out"
