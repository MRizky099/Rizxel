#!/bin/sh
# Builds Rizxel-<version>-x86_64.AppImage from a release build.
#   packaging/appimage/build.sh <version> <appimagetool>
# Needs target/release/rizxel and THIRD_PARTY_LICENSES.html at the repo root.
set -eu
VERSION="$1"
TOOL="$2"
ID=io.github.mrizky099.rizxel
APPDIR=target/Rizxel.AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/mime/packages" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
         "$APPDIR/usr/share/icons/hicolor/256x256/mimetypes" \
         "$APPDIR/usr/share/doc/rizxel/craft-ui"
install -m 755 target/release/rizxel "$APPDIR/usr/bin/rizxel"
install -m 755 packaging/appimage/AppRun "$APPDIR/AppRun"
cp packaging/appimage/$ID.desktop "$APPDIR/$ID.desktop"
cp packaging/appimage/$ID.desktop "$APPDIR/usr/share/applications/$ID.desktop"
cp packaging/appimage/$ID.xml "$APPDIR/usr/share/mime/packages/$ID.xml"
cp assets/Rizxel_icon_256x256.png "$APPDIR/$ID.png"
cp assets/Rizxel_icon_256x256.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/$ID.png"
cp assets/rizxel_document.png "$APPDIR/usr/share/icons/hicolor/256x256/mimetypes/$ID.project.png"
# License files next to the binary (and at the AppDir root).
cp LICENSE THIRD_PARTY_LICENSES.html README.md "$APPDIR/usr/share/doc/rizxel/"
cp LICENSE THIRD_PARTY_LICENSES.html "$APPDIR/"
cp vendor/craft-ui/LICENSE-MIT vendor/craft-ui/LICENSE-APACHE vendor/craft-ui/NOTICE \
   "$APPDIR/usr/share/doc/rizxel/craft-ui/"
# No FUSE on CI runners: the tool runs extracted.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$TOOL" "$APPDIR" "Rizxel-$VERSION-x86_64.AppImage"
