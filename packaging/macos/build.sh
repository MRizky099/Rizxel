#!/bin/sh
# Builds Rizxel-<version>-macos-universal.dmg (macOS only: lipo, sips,
# iconutil, hdiutil). Run after building both release targets:
#   cargo build --release --target x86_64-apple-darwin
#   cargo build --release --target aarch64-apple-darwin
#   packaging/macos/build.sh <version>
# Needs THIRD_PARTY_LICENSES.html at the repo root.
set -eu
VERSION="$1"
OUT=target/macos
APP="$OUT/Rizxel.app"
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/craft-ui"
lipo -create -output "$APP/Contents/MacOS/rizxel" \
  target/x86_64-apple-darwin/release/rizxel \
  target/aarch64-apple-darwin/release/rizxel
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# .icns from the PNG icons.
icns() { # <png> <name>
  set=$OUT/$2.iconset
  mkdir -p "$set"
  for s in 16 32 128 256; do
    sips -z $s $s "$1" --out "$set/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    sips -z $d $d "$1" --out "$set/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$set" -o "$APP/Contents/Resources/$2.icns"
}
icns assets/Rizxel_icon.png Rizxel
icns assets/Rizxel_fancy_document_icon.png RizxelDocument

cp LICENSE THIRD_PARTY_LICENSES.html README.md "$APP/Contents/Resources/"
cp vendor/craft-ui/LICENSE-MIT vendor/craft-ui/LICENSE-APACHE vendor/craft-ui/NOTICE \
   "$APP/Contents/Resources/craft-ui/"

# The disk image: the app, a link to /Applications and the license files.
STAGE=$OUT/dmg
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp LICENSE THIRD_PARTY_LICENSES.html "$STAGE/"
hdiutil create -volname Rizxel -srcfolder "$STAGE" -ov -format UDZO \
  "Rizxel-$VERSION-macos-universal.dmg"
