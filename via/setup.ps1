# Windows version of setup.sh: puts ViaProxy + the PumpkinForward plugin in front of Pumpkin.
#   powershell -ExecutionPolicy Bypass -File setup.ps1 [-Port 25565] [-BackendPort 25566]
# Afterwards: start server\start.bat, then via\start-via.bat. Players connect to -Port as before.
param([int]$Port = 25565, [int]$BackendPort = 25566)
$ErrorActionPreference = "Stop"
$Here = $PSScriptRoot
$Root = (Resolve-Path "$Here\..").Path
Get-Content "$Here\viaproxy.env" | Where-Object { $_ -match '^\s*([A-Z_]+)=(.*)$' } | ForEach-Object {
    $name, $value = $Matches[1], $Matches[2].Trim('"')
    $value = $value -replace '\$\{VIAPROXY_VERSION\}', $env:VIAPROXY_VERSION
    Set-Item -Path "env:$name" -Value $value
}

if (-not (Get-Command java -ErrorAction SilentlyContinue)) { throw "java not found. ViaProxy needs Java 17+ (https://adoptium.net)." }
$javaMajor = [int]((& java -version 2>&1 | Select-String 'version "(\d+)' | Select-Object -First 1).Matches.Groups[1].Value)
if ($javaMajor -lt 17) { throw "Java 17 or newer is required (found $javaMajor)." }

$Run = "$Here\run"
New-Item -ItemType Directory -Force "$Run\plugins\PumpkinForward" | Out-Null

# ViaProxy
$Jar = "$Run\ViaProxy-$env:VIAPROXY_VERSION.jar"
if (-not (Test-Path $Jar) -or (Get-FileHash $Jar -Algorithm SHA256).Hash.ToLower() -ne $env:VIAPROXY_SHA256) {
    Write-Host "Downloading ViaProxy $env:VIAPROXY_VERSION..."
    Invoke-WebRequest -Uri $env:VIAPROXY_URL -OutFile $Jar
    if ((Get-FileHash $Jar -Algorithm SHA256).Hash.ToLower() -ne $env:VIAPROXY_SHA256) { Remove-Item $Jar; throw "ViaProxy jar hash mismatch" }
}
Get-ChildItem "$Run\ViaProxy-*.jar" | Where-Object { $_.FullName -ne $Jar } | Remove-Item

# PumpkinForward plugin
$Plugin = @("$Here\PumpkinForward.jar", "$Here\plugin\build\PumpkinForward.jar") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Plugin) {
    if (Get-Command javac -ErrorAction SilentlyContinue) {
        Write-Host "Building the PumpkinForward plugin..."
        & powershell -ExecutionPolicy Bypass -File "$Here\plugin\build.ps1" -ViaProxyJar $Jar
        if ($LASTEXITCODE -ne 0) { throw "plugin build failed" }
        $Plugin = "$Here\plugin\build\PumpkinForward.jar"
    } else {
        Write-Host "Downloading the PumpkinForward plugin from the latest ServerJar release..."
        Invoke-WebRequest -Uri "https://github.com/jayclrk358/ServerJar/releases/latest/download/PumpkinForward.jar" -OutFile "$Here\PumpkinForward.jar"
        $Plugin = "$Here\PumpkinForward.jar"
    }
}
Copy-Item $Plugin "$Run\plugins\PumpkinForward.jar" -Force

# Shared secret (kept if it already exists)
$SecretFile = "$Run\plugins\PumpkinForward\secret.txt"
if (-not (Test-Path $SecretFile) -or -not (Get-Content $SecretFile -Raw).Trim()) {
    $bytes = New-Object byte[] 24
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $secret = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
    [IO.File]::WriteAllText($SecretFile, "$secret`n")
}
$Secret = (Get-Content $SecretFile -Raw).Trim()

# ViaProxy config (only written if missing)
if (-not (Test-Path "$Run\viaproxy.yml")) {
    (Get-Content "$Here\viaproxy.yml") `
        -replace '^bind-address:.*', "bind-address: `"0.0.0.0:$Port`"" `
        -replace '^target-address:.*', "target-address: `"127.0.0.1:$BackendPort`"" |
        Set-Content "$Run\viaproxy.yml" -Encoding UTF8
}

# Pumpkin config
$Toml = "$Root\server\pumpkin.toml"
if (-not (Test-Path $Toml)) { throw "server\pumpkin.toml not found at $Toml" }
if (-not (Test-Path "$Toml.before-via")) { Copy-Item $Toml "$Toml.before-via" }
$escaped = $Secret.Replace('\', '\\').Replace('"', '\"')
$wanted = @{
    "networking.java"           = [ordered]@{ "address" = "`"127.0.0.1:$BackendPort`""; "online_mode" = "false" }
    "networking.proxy"          = [ordered]@{ "enabled" = "true" }
    "networking.proxy.velocity" = [ordered]@{ "enabled" = "true"; "secret" = "`"$escaped`"" }
}
$out = New-Object System.Collections.Generic.List[string]
$pending = [ordered]@{}
function Flush { foreach ($k in @($pending.Keys)) { $out.Add("$k = $($pending[$k])") }; $script:pending = [ordered]@{} }
foreach ($line in Get-Content $Toml) {
    if ($line -match '^\s*\[([^\]]+)\]\s*$') {
        Flush
        $section = $Matches[1].Trim()
        $pending = [ordered]@{}
        if ($wanted.Contains($section)) { foreach ($k in $wanted[$section].Keys) { $pending[$k] = $wanted[$section][$k] } }
        $out.Add($line); continue
    }
    if ($line -match '^\s*([A-Za-z0-9_]+)\s*=' -and $pending.Contains($Matches[1])) {
        $out.Add("$($Matches[1]) = $($pending[$Matches[1]])"); $pending.Remove($Matches[1]); continue
    }
    $out.Add($line)
}
Flush
foreach ($name in $wanted.Keys) {
    if (-not ($out -contains "[$name]")) { $out.Add(""); $out.Add("[$name]"); foreach ($k in $wanted[$name].Keys) { $out.Add("$k = $($wanted[$name][$k])") } }
}
[IO.File]::WriteAllText($Toml, ($out -join "`n") + "`n")

Write-Host ""
Write-Host "Done."
Write-Host "  ViaProxy:  via\run\ViaProxy-$env:VIAPROXY_VERSION.jar  (config: via\run\viaproxy.yml, listens on :$Port)"
Write-Host "  Plugin:    via\run\plugins\PumpkinForward.jar  (secret: via\run\plugins\PumpkinForward\secret.txt)"
Write-Host "  Pumpkin:   server\pumpkin.toml now listens on 127.0.0.1:$BackendPort (backup: pumpkin.toml.before-via)"
Write-Host ""
Write-Host "Start Pumpkin (server\start.bat), then ViaProxy (via\start-via.bat). Only open port $Port in the firewall."
Write-Host "Clients from Minecraft 1.7.2 up to 26.3 can now join."
