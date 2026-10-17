# Signs one Windows program with Azure Artifact Signing. Tauri runs this for
# the app, the stackyard command and the installers (bundle.windows.signCommand,
# set by the Installers workflow only when the signing secrets exist).
#
# Needs: AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET (an app
# registration with the "Artifact Signing Certificate Profile Signer" role)
# and SIGNING_DLIB / SIGNING_METADATA, which the workflow prepares.
param([Parameter(Mandatory = $true)][string]$File)
$ErrorActionPreference = "Stop"

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signtool) { throw "signtool.exe was not found" }

# The signing service is reached over the network; a dropped connection
# is worth one more try before the build fails.
for ($try = 1; $try -le 3; $try++) {
  & $signtool.FullName sign /v /fd SHA256 /tr "http://timestamp.acs.microsoft.com" /td SHA256 `
    /dlib $env:SIGNING_DLIB /dmdf $env:SIGNING_METADATA $File
  if ($LASTEXITCODE -eq 0) { exit 0 }
  Start-Sleep -Seconds (5 * $try)
}
throw "could not sign $File"
