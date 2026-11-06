@echo off
rem Starts ViaProxy in headless mode from via\run. Start Pumpkin (server\start.bat) first.
cd /d "%~dp0run" || (echo via\run not found: run setup.ps1 first & exit /b 1)
for %%f in (ViaProxy-*.jar) do set JAR=%%f
if "%JAR%"=="" (echo No ViaProxy jar in via\run: run setup.ps1 first & exit /b 1)
java -Xms64M -Xmx512M -XX:+UseSerialGC -DskipUpdateCheck=true -jar "%JAR%" cli
