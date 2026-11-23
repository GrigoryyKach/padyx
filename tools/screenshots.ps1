# Screenshots of the Padyx window with the demo pad (no hardware, own temporary data folder, own window only).
# Usage: .\tools\screenshots.ps1 -Out .\assets\screenshots -Theme dark -Lang en -Navs overview,sticks -DemoT 25.5
# -AutoTest drift|circ starts that stick test at launch; use -Hold:$false and a large -Wait so it can finish.
param(
    [string]$Exe = (Join-Path $PSScriptRoot "..\target\release\Padyx.exe"),
    [string]$Out = (Join-Path $PSScriptRoot "..\assets\screenshots"),
    [string]$Theme = "dark",
    [string]$Lang = "en",
    [string]$Navs = "overview,sticks,triggers,buttons,motion,light,settings",
    [string]$Prefix = "",
    [double]$DemoT = 25.5,
    [bool]$Hold = $true,
    [int]$Wait = 40,
    [string]$AutoTest = "",
    [string]$Window = "1280x900"
)
$ErrorActionPreference = "Stop"
$data = Join-Path ([IO.Path]::GetTempPath()) ("padyx-shots-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $data | Out-Null
if (-not $Prefix) { $Prefix = "$Theme-$Lang" }
$env:PADYX_DATA_DIR = $data
$env:PADYX_SHOT_DIR = (Resolve-Path (New-Item -ItemType Directory -Force $Out)).Path
$env:PADYX_SHOT_PREFIX = $Prefix
$env:PADYX_SHOT_NAVS = $Navs
$env:PADYX_SHOT_WAIT = "$Wait"
$env:PADYX_THEME = $Theme
$env:PADYX_LANG = $Lang
$env:PADYX_DEMO_T = "$DemoT"
$env:PADYX_WINDOW = $Window
if ($Hold) { $env:PADYX_DEMO_HOLD = "1" } else { Remove-Item Env:PADYX_DEMO_HOLD -ErrorAction SilentlyContinue }
if ($AutoTest) { $env:PADYX_AUTOTEST = $AutoTest } else { Remove-Item Env:PADYX_AUTOTEST -ErrorAction SilentlyContinue }
# Only our own process is started, waited for and (if it hangs) killed.
$p = Start-Process -FilePath $Exe -ArgumentList "--demo" -PassThru
if (-not $p.WaitForExit(120000)) { $p.Kill(); Write-Host "killed after timeout" }
Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
Get-ChildItem $env:PADYX_SHOT_DIR -Filter "$Prefix-*.png" | Select-Object Name, Length
