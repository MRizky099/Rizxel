# Writes LICENSE as RTF for the MSI's license page.
#   pwsh packaging/wix/license-rtf.ps1 <LICENSE> <out.rtf>
param([string]$In, [string]$Out)
$text = Get-Content -Raw -Encoding utf8 $In
$text = $text.Replace('\', '\').Replace('{', '\{').Replace('}', '\}')
$body = ($text -split "`r?`n") -join "\par`n"
$rtf = "{\rtf1\ansi\deff0{\fonttbl{\f0 Consolas;}}\fs16`n$body`n}"
[System.IO.File]::WriteAllText($Out, $rtf, [System.Text.Encoding]::ASCII)
