# Builds Rizxel-<version>-windows-x64.zip and Rizxel-<version>-windows-x64.msi
# from a release build.
#   pwsh packaging/wix/build.ps1 -Version 0.10.0 [-NoMsi]
# Needs target/release/rizxel.exe and THIRD_PARTY_LICENSES.html at the repo
# root; the MSI also needs the WiX Toolset 5 `wix` tool with the UI extension:
#   dotnet tool install --global wix --version 5.0.2
#   wix extension add --global WixToolset.UI.wixext/5.0.2
param([Parameter(Mandatory)][string]$Version, [switch]$NoMsi)
$ErrorActionPreference = 'Stop'
$stage = 'target/package/Rizxel'
Remove-Item -Recurse -Force target/package -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/craft-ui" | Out-Null
Copy-Item target/release/rizxel.exe, LICENSE, THIRD_PARTY_LICENSES.html, README.md $stage
Copy-Item vendor/craft-ui/LICENSE-MIT, vendor/craft-ui/LICENSE-APACHE, vendor/craft-ui/NOTICE "$stage/craft-ui"

# Portable zip: the license files sit at its root next to rizxel.exe.
$zip = "Rizxel-$Version-windows-x64.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage/*" -DestinationPath $zip
Write-Output "wrote $zip"

if (-not $NoMsi) {
    & "$PSScriptRoot/license-rtf.ps1" LICENSE "$stage/License.rtf"
    $msi = "Rizxel-$Version-windows-x64.msi"
    wix build packaging/wix/main.wxs -ext WixToolset.UI.wixext `
        -d "Version=$Version" -d "Stage=$stage" -d "Assets=assets" -o $msi
    if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
    Write-Output "wrote $msi"
}
