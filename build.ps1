param(
    [ValidateSet('check','test','test-release','desktop-test','desktop-test-release','build','release','clippy','fmt')]
    [string]$Mode = 'release'
)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:Path = "$cargoBin;$env:Path"
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw '缺少 Visual Studio Build Tools（MSVC 和 Windows SDK）。' }
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { throw '未找到完成安装的 MSVC 工具链。' }
$devCmd = Join-Path $vsPath 'Common7\Tools\VsDevCmd.bat'
if ($env:VSCMD_ARG_TGT_ARCH -ne 'x64' -or -not $env:VCToolsInstallDir) {
    # Stray quotes in inherited PATH break VsDevCmd's batch conditionals.
    # Normalize only this build process, never the user's saved environment.
    $env:Path = $env:Path.Replace('"', '')
    [Environment]::SetEnvironmentVariable('__VSCMD_PREINIT_PATH', $null, 'Process')
    $command = '"' + $devCmd + '" -no_logo -arch=x64 -host_arch=x64 >nul && set'
    $compilerEnvironment = & $env:ComSpec /d /s /c $command
    if ($LASTEXITCODE -ne 0) { throw 'Visual Studio编译环境初始化失败。' }
    $compilerEnvironment | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
    }
}
$env:Path = "$cargoBin;$env:Path"
$nativeCmake = Join-Path $vsPath 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
if (Test-Path -LiteralPath $nativeCmake) { $env:Path = "$nativeCmake;$env:Path" }
$cargoArgs = switch ($Mode) {
    'check' { @('check','--locked') }
    'test' { @('test','--locked','--','--test-threads=1') }
    'test-release' { @('test','--release','--locked','--','--test-threads=1') }
    'desktop-test' { @('test','--locked','--features','desktop-acceptance','--test','desktop_acceptance') }
    'desktop-test-release' { @('test','--release','--locked','--features','desktop-acceptance','--test','desktop_acceptance') }
    'build' { @('build','--locked') }
    'release' { @('build','--release','--locked') }
    'clippy' { @('clippy','--locked','--all-targets','--all-features','--','-D','warnings') }
    'fmt' { @('fmt','--','--check') }
}
& (Join-Path $cargoBin 'cargo.exe') @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($Mode -eq 'release') {
    New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
    Copy-Item -LiteralPath 'target\x86_64-pc-windows-msvc\release\window-scaler.exe' -Destination 'dist\WindowScaler.exe'
    Copy-Item -LiteralPath 'README.md' -Destination 'dist\使用说明.md'
    Get-FileHash -LiteralPath 'dist\WindowScaler.exe' -Algorithm SHA256
}
