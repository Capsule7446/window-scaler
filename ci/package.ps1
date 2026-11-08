param([Parameter(Mandatory)][ValidatePattern('^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')][string]$Version)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Set-Location -LiteralPath $projectRoot
$packageBlock = ((Get-Content -Raw -LiteralPath Cargo.toml) -split '\[package\]', 2)[1] -split '\r?\n\[', 2
if ($packageBlock[0] -notmatch '(?m)^version\s*=\s*"([^"]+)"' -or $Matches[1] -ne $Version) { throw '打包版本与Cargo.toml不一致。' }
$binary = Join-Path $projectRoot 'dist\WindowScaler.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw '请先运行build.ps1 -Mode release。' }
$commit = git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw '无法读取构建提交。' }
$outputRoot = Join-Path $projectRoot 'artifacts'
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$stage = Join-Path $outputRoot ('staging-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'WindowScaler.exe')
    Copy-Item -LiteralPath README.md,GLOSSARY.md,CHANGELOG.md -Destination $stage
    Copy-Item -LiteralPath docs -Destination (Join-Path $stage 'docs') -Recurse
    $info = [ordered]@{version=$Version;commit=$commit.Trim();target='x86_64-pc-windows-msvc';created_utc=[DateTime]::UtcNow.ToString('o')}
    $info | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $stage 'build-info.json') -Encoding utf8NoBOM
    $zipName = "WindowScaler-v$Version-windows-x86_64.zip"
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $outputRoot $zipName) -Force
    Copy-Item -LiteralPath (Join-Path $stage 'WindowScaler.exe'),(Join-Path $stage 'build-info.json') -Destination $outputRoot -Force
    $checksums = foreach ($name in @('WindowScaler.exe',$zipName,'build-info.json')) {
        $hash = (Get-FileHash -LiteralPath (Join-Path $outputRoot $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $name"
    }
    $checksums | Set-Content -LiteralPath (Join-Path $outputRoot 'SHA256SUMS.txt') -Encoding utf8NoBOM
    $checksums
} finally {
    $resolvedStage = [IO.Path]::GetFullPath($stage)
    $allowedPrefix = [IO.Path]::GetFullPath($outputRoot).TrimEnd('\') + '\'
    if (-not $resolvedStage.StartsWith($allowedPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw '临时打包目录超出artifacts，停止清理。' }
    Remove-Item -LiteralPath $resolvedStage -Recurse -Force
}
