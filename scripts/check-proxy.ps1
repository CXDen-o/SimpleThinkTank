$ErrorActionPreference = 'Stop'
$zip = "$env:TEMP\nsis-mirror.zip"
$dll = "$env:TEMP\nsis_tauri_utils.dll"
$extractTo = "$env:TEMP\nsis-mirror-extract"
$nsisHome = "$env:LOCALAPPDATA\tauri\NSIS"

# 1. SHA1 校验(官方 tauri-cli 2.11.4 常量)
$h1 = (Get-FileHash $zip -Algorithm SHA1).Hash
"zip  SHA1: $h1"
if ($h1 -ne 'EF7FF767E5CBD9EDD22ADD3A32C9B8F4500BB10D') { throw 'zip SHA1 不匹配,可能已被篡改,中止' }

$h2 = (Get-FileHash $dll -Algorithm SHA1).Hash
"dll  SHA1: $h2"
if ($h2 -ne '75197FEE3C6A814FE035788D1C34EAD39349B860') { throw 'dll SHA1 不匹配,可能已被篡改,中止' }

# 2. 重新解压(zip 已验证为官方原版)
if (Test-Path $extractTo) { Remove-Item -Recurse -Force $extractTo }
Expand-Archive -Path $zip -DestinationPath $extractTo -Force
$makensis = Get-ChildItem -Path $extractTo -Recurse -Filter makensis.exe | Select-Object -First 1
$srcRoot = $makensis.Directory.FullName

# 3. 部署 NSIS
if (Test-Path $nsisHome) { Remove-Item -Recurse -Force $nsisHome }
New-Item -ItemType Directory -Force -Path $nsisHome | Out-Null
Copy-Item -Path (Join-Path $srcRoot '*') -Destination $nsisHome -Recurse -Force

# 4. 部署插件到 x86-unicode
$pluginDir = Join-Path $nsisHome 'Plugins\x86-unicode'
New-Item -ItemType Directory -Force -Path $pluginDir | Out-Null
Copy-Item $dll (Join-Path $pluginDir 'nsis_tauri_utils.dll') -Force

# 5. 复核
& (Join-Path $nsisHome 'makensis.exe') /VERSION
Get-ChildItem $pluginDir | Select-Object Name, Length
'done'
