# Builds the PumpkinForward ViaProxy plugin with plain javac (no Gradle needed).
# Needs a JDK 17+. Output: build\PumpkinForward.jar
#   powershell -ExecutionPolicy Bypass -File build.ps1 [-ViaProxyJar path\to\ViaProxy.jar]
param([string]$ViaProxyJar = "")
$ErrorActionPreference = "Stop"
$Here = $PSScriptRoot
Get-Content "$Here\..\viaproxy.env" | Where-Object { $_ -match '^\s*([A-Z_]+)=(.*)$' } | ForEach-Object {
    $name, $value = $Matches[1], $Matches[2].Trim('"')
    $value = $value -replace '\$\{VIAPROXY_VERSION\}', $env:VIAPROXY_VERSION
    Set-Item -Path "env:$name" -Value $value
}
if (-not $ViaProxyJar) { $ViaProxyJar = "$Here\build\ViaProxy-$env:VIAPROXY_VERSION.jar" }
if (-not (Get-Command javac -ErrorAction SilentlyContinue)) { throw "javac not found: install a JDK 17 or newer" }
New-Item -ItemType Directory -Force "$Here\build\classes" | Out-Null
if (-not (Test-Path $ViaProxyJar)) {
    Write-Host "Downloading ViaProxy $env:VIAPROXY_VERSION (compile dependency)..."
    Invoke-WebRequest -Uri $env:VIAPROXY_URL -OutFile $ViaProxyJar
}
if ((Get-FileHash $ViaProxyJar -Algorithm SHA256).Hash.ToLower() -ne $env:VIAPROXY_SHA256) { throw "ViaProxy jar hash mismatch" }
Remove-Item -Recurse -Force "$Here\build\classes\*" -ErrorAction SilentlyContinue
$sources = Get-ChildItem -Recurse "$Here\src\main\java" -Filter *.java | ForEach-Object { $_.FullName }
& javac --release 17 -proc:none -Xlint:all -Werror -cp $ViaProxyJar -d "$Here\build\classes" @sources
if ($LASTEXITCODE -ne 0) { throw "javac failed" }
Copy-Item "$Here\src\main\resources\viaproxy.yml" "$Here\build\classes\"
& jar --create --file "$Here\build\PumpkinForward.jar" -C "$Here\build\classes" .
if ($LASTEXITCODE -ne 0) { throw "jar failed" }
Write-Host "Built: $Here\build\PumpkinForward.jar"
