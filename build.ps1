# Builds the optimised Pumpkin server:
#   1. clones Pumpkin at the pinned commit (first run only)
#   2. applies everything in patches\
#   3. downloads the vanilla server jar Pumpkin needs for its game data (hash-checked)
#   4. builds a release binary tuned for this CPU and copies it to server\
#
# Usage:  powershell -ExecutionPolicy Bypass -File build.ps1 [-Portable]
#   -Portable   build for any x86-64 CPU instead of this machine's (use when the
#               binary will run on a different PC/host)
param(
    [switch]$Portable,
    [string]$PumpkinRepo = 'https://github.com/Pumpkin-MC/Pumpkin.git',
    [string]$PumpkinCommit = 'd1190dc51edc3d4447780017e244f8bc882cb81a',
    [string]$MinecraftVersion = '26.3'
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$src = Join-Path $root 'Pumpkin'
$env:Path += ";$env:USERPROFILE\.cargo\bin"

function Run($exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "$exe $($argv -join ' ') failed with exit code $LASTEXITCODE" }
}

foreach ($tool in 'git', 'cargo') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool not found. Install it first (Rust: https://rustup.rs)." }
}

# 1. source
if (-not (Test-Path $src)) {
    Write-Host "Cloning Pumpkin..."
    Run git @('clone', '--recurse-submodules', $PumpkinRepo, $src)
    Run git @('-C', $src, 'checkout', '--quiet', $PumpkinCommit)
    Run git @('-C', $src, 'submodule', 'update', '--init', '--recursive')
}

# 2. patches (skip ones that are already applied)
foreach ($p in Get-ChildItem (Join-Path $root 'patches') -Filter *.patch | Sort-Object Name) {
    & git -C $src apply --reverse --check $p.FullName 2>$null
    if ($LASTEXITCODE -eq 0) { Write-Host "Already applied: $($p.Name)"; continue }
    Write-Host "Applying: $($p.Name)"
    Run git @('-C', $src, 'apply', '--whitespace=nowarn', $p.FullName)
}

# 3. vanilla server jar (Pumpkin extracts the vanilla datapack from it at build time)
$vanillaDir = Join-Path $root 'vanilla'
$jar = Join-Path $vanillaDir "server-$MinecraftVersion.jar"
New-Item -ItemType Directory -Force $vanillaDir | Out-Null
$manifest = Invoke-RestMethod 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'
$entry = $manifest.versions | Where-Object id -eq $MinecraftVersion | Select-Object -First 1
if (-not $entry) { throw "Minecraft $MinecraftVersion not found in Mojang's version manifest" }
$server = (Invoke-RestMethod $entry.url).downloads.server
if (-not (Test-Path $jar) -or (Get-FileHash $jar -Algorithm SHA1).Hash -ne $server.sha1.ToUpper()) {
    Write-Host "Downloading vanilla $MinecraftVersion server jar..."
    Run curl.exe @('-L', '--retry', '5', '--fail', '-o', $jar, $server.url)
    if ((Get-FileHash $jar -Algorithm SHA1).Hash -ne $server.sha1.ToUpper()) { throw 'Vanilla jar hash mismatch' }
}
$env:PUMPKIN_MINECRAFT_SERVER_JAR = $jar

# 4. build
$env:RUSTFLAGS = if ($Portable) { '' } else { '-C target-cpu=native' }
Write-Host "Building (release, LTO; this takes a while)..."
Push-Location $src
try { Run cargo @('build', '--release', '-p', 'pumpkin') } finally { Pop-Location }

$exe = Join-Path $src 'target\release\pumpkin.exe'
Copy-Item $exe (Join-Path $root 'server\pumpkin.exe') -Force
Write-Host "Done: server\pumpkin.exe  (start it with server\start.bat)"
