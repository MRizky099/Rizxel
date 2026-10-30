#!/bin/sh
# Writes the GitHub Release notes for a version to stdout: its CHANGELOG
# section, a download link per package, the install notes for the unsigned
# builds, the license and the source link.
#
#   sh packaging/release_notes.sh 0.10.0 MRizky099/Rizxel dist > notes.md
#
# The package list comes from the files in the third argument (the release
# workflow's downloaded artifacts).
set -eu

version="$1"
repo="$2"
dist="$3"
tag="v$version"
base="https://github.com/$repo/releases/download/$tag"

# The "## [x.y.z] — date" section, without its heading, up to the next one.
section=$(awk -v v="$version" '
    index($0, "## [" v "]") == 1 { on = 1; next }
    on && /^## \[/ { exit }
    on { print }
' docs/CHANGELOG.md)
if [ -z "$(printf '%s' "$section" | tr -d '[:space:]')" ]; then
    echo "release_notes.sh: no CHANGELOG section for $version" >&2
    exit 1
fi

describe() {
    case "$1" in
        *.msi) echo "Windows 10/11 installer (per user, no admin rights, opens \`.rzxl\` files)" ;;
        *windows*.zip) echo "Windows 10/11 portable: unzip and run \`rizxel.exe\`" ;;
        *.AppImage) echo "Linux x86-64: \`chmod +x\` and run" ;;
        *.dmg) echo "macOS 11+ universal (Apple silicon and Intel): drag Rizxel to *Applications*" ;;
        *samples.zip) echo "Sample models made with Rizxel (\`.rzxl\`, CC0)" ;;
        *) echo "" ;;
    esac
}

printf '%s\n' "$section" | sed -e '/./,$!d'
cat <<EOF

## Downloads

| Package | Platform |
|---|---|
EOF
for f in "$dist"/*; do
    name=$(basename "$f")
    printf '| [%s](%s/%s) | %s |\n' "$name" "$base" "$name" "$(describe "$name")"
done
cat <<EOF

The [user guide](https://mrizky099.github.io/Rizxel/) explains every tool, panel and
format.

### Unsigned builds

This beta is not code-signed, so the system warns on the first start:

- **Windows** (SmartScreen, "Windows protected your PC"): click *More info*, then *Run
  anyway*.
- **macOS** (Gatekeeper): right-click Rizxel in *Applications*, choose *Open*, then
  *Open* again (or *System Settings ▸ Privacy & Security ▸ Open Anyway*).
- **Linux**: \`chmod +x Rizxel-$version-x86_64.AppImage\`, then run it.

## License and source

Rizxel is free software under the **GNU General Public License v3.0 or later**
(GPL-3.0-or-later), without any warranty. Every package contains \`LICENSE\` and
\`THIRD_PARTY_LICENSES.html\`. The complete corresponding source code of these binaries
is the tagged source: https://github.com/$repo/tree/$tag (also attached as *Source code*
zip and tar.gz).
EOF
