# Screenshots of the Padyx window with the demo pad (no hardware, own temporary data folder, own window only).
# Usage: .\tools\screenshots.ps1 -Out .\assets\screenshots -Theme dark -Lang en -Navs overview,sticks -DemoT 25.5
# -AutoTest drift|circ starts that stick test at launch; use -Hold:$false and a large -Wait so it can finish.
# -Sample is like -Emulation but writes tools/sample-config.json (remap, macros, turbo, three profiles), pretends
#   the game forza.exe is in front (PADYX_FG_MOCK, so auto-switch picks the Racing profile) and opens the profile
#   named by -EditProfile on the pages that edit one. No real window is read.
# -Emulation turns the Xbox emulation on with the mock back end (nothing real is created or hidden) and writes
#   dead zones into the temporary config so the page shows something; -Scroll scrolls every page by that many pixels.
param(
    [string]$Exe = (Join-Path $PSScriptRoot "..\target\release\Padyx.exe"),
    [string]$Out = (Join-Path $PSScriptRoot "..\assets\screenshots"),
    [string]$Theme = "dark",
    [string]$Lang = "en",
    [string]$Navs = "overview,sticks,triggers,buttons,motion,light,emulation,settings",
    [string]$Prefix = "",
    [double]$DemoT = 25.5,
    [bool]$Hold = $true,
    [int]$Wait = 40,
    [string]$AutoTest = "",
    [string]$Window = "1280x900",
    [switch]$Emulation,
    [switch]$Sample,
    [string]$EditProfile = "",
    [int]$Scroll = 0
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
if ($Scroll -gt 0) { $env:PADYX_SCROLL = "$Scroll" } else { Remove-Item Env:PADYX_SCROLL -ErrorAction SilentlyContinue }
$utf8 = New-Object System.Text.UTF8Encoding($false)
if ($Emulation -or $Sample) {
    $env:PADYX_EMU_MOCK = "1"
    [IO.File]::WriteAllText((Join-Path $data "emulation.json"), '{"enabled":true}', $utf8)
    if ($Sample) {
        Copy-Item (Join-Path $PSScriptRoot "sample-config.json") (Join-Path $data "config.json")
        $env:PADYX_FG_MOCK = "C:\Games\forza.exe"
        if ($EditProfile) { $env:PADYX_EDIT_PROFILE = $EditProfile }
    }
    $cfg = '{"onboarded":true,"emu":{"left":{"inner":0.12,"outer":0.03,"anti":0.0,"curve":"Smooth"},"right":{"inner":0.08,"outer":0.0,"anti":0.0,"curve":"Custom","p1":[0.3,0.18],"p2":[0.7,0.82]},"lt":{"inner":0.04,"outer":0.05},"rumble_pct":80}}'
    if (-not $Sample) { [IO.File]::WriteAllText((Join-Path $data "config.json"), $cfg, $utf8) }
} else {
    Remove-Item Env:PADYX_EMU_MOCK -ErrorAction SilentlyContinue
}
# Only our own process is started, waited for and (if it hangs) killed.
$p = Start-Process -FilePath $Exe -ArgumentList "--demo" -PassThru
if (-not $p.WaitForExit(120000)) { $p.Kill(); Write-Host "killed after timeout" }
Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
Remove-Item Env:PADYX_EMU_MOCK, Env:PADYX_SCROLL, Env:PADYX_FG_MOCK, Env:PADYX_EDIT_PROFILE -ErrorAction SilentlyContinue
Get-ChildItem $env:PADYX_SHOT_DIR -Filter "$Prefix-*.png" | Select-Object Name, Length
