[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Version = $env:VERSION,

    [Parameter(Position = 1)]
    [string]$Target = $env:TARGET,

    [Parameter(Position = 2)]
    [string]$OutDir = $(if ($env:OUT_DIR) { $env:OUT_DIR } else { "dist" }),

    [string]$BuildTool = $(if ($env:BUILD_TOOL) { $env:BUILD_TOOL } else { "cargo" }),

    [switch]$RunGate,

    [switch]$GateOnly,

    [switch]$BuildOnly,

    [switch]$PackageOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Show-Usage {
    @"
Usage:
  `$env:TARGET='x86_64-pc-windows-msvc'; .\scripts\release.ps1
    .\scripts\release.ps1 -Version v0.1.0-alpha.2 -Target x86_64-pc-windows-msvc -OutDir dist

Inputs:
  VERSION / arg1   optional semver version or tag; defaults to the Cargo package tag
  TARGET  / arg2   Rust target triple to package
  OUT_DIR / arg3   output directory (default: dist)
  BUILD_TOOL       cargo or cross (default: cargo)
  RUN_GATE         run cargo fmt/clippy/check/test before packaging
  GATE_ONLY        run the gate and exit without building or packaging
  BUILD_ONLY       build the release binary and exit without packaging
  PACKAGE_ONLY     package an existing binary at target/<target>/release
"@
}

function Get-RequiredCommand {
    param([Parameter(Mandatory = $true)][string]$Name)

    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue
    if ($null -eq $command) {
        throw "FAIL: $Name is required"
    }

    return $command.Source
}

function Get-NormalizedReleaseTag {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $null
    }

    if ($Value.StartsWith("v")) {
        return $Value
    }

    return "v$Value"
}

function Get-ExpectedReleaseTag {
    param([Parameter(Mandatory = $true)][string]$CargoPath)

    $pkgId = & $CargoPath pkgid --quiet
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($pkgId)) {
        throw "FAIL: could not read the Cargo package version via 'cargo pkgid --quiet'"
    }

    $fragment = $pkgId.Substring($pkgId.LastIndexOf("#") + 1)
    if ([string]::IsNullOrWhiteSpace($fragment)) {
        throw "FAIL: unexpected cargo pkgid output: $pkgId"
    }

    $atIndex = $fragment.LastIndexOf("@")
    if ($atIndex -ge 0) {
        $fragment = $fragment.Substring($atIndex + 1)
    }

    if ([string]::IsNullOrWhiteSpace($fragment)) {
        throw "FAIL: unexpected cargo pkgid output: $pkgId"
    }

    return "v" + $fragment
}

function Assert-ReleaseMode {
    $modeCount = @($GateOnly, $BuildOnly, $PackageOnly) | Where-Object { $_ } | Measure-Object | Select-Object -ExpandProperty Count
    if ($modeCount -gt 1) {
        throw "FAIL: GateOnly, BuildOnly, and PackageOnly are mutually exclusive"
    }

    if ($GateOnly -and -not $RunGate) {
        throw "FAIL: GateOnly requires RunGate"
    }

    if ($PackageOnly -and $RunGate) {
        throw "FAIL: PackageOnly cannot be combined with RunGate"
    }
}

function Get-LatestDirectory {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path $Path)) {
        throw "FAIL: expected directory not found: $Path"
    }

    $directory = Get-ChildItem -Path $Path -Directory | Sort-Object Name -Descending | Select-Object -First 1
    if ($null -eq $directory) {
        throw "FAIL: no version directories found under $Path"
    }

    return $directory
}

function Get-VsInstallRoot {
    $vswherePath = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswherePath) {
        $installRoot = & $vswherePath -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1
        if (-not [string]::IsNullOrWhiteSpace($installRoot)) {
            return $installRoot.Trim()
        }
    }

    foreach ($candidate in @(
        "C:\BuildTools",
        "C:\Program Files\Microsoft Visual Studio\2022\BuildTools",
        "C:\Program Files\Microsoft Visual Studio\2022\Community"
    )) {
        if (Test-Path (Join-Path $candidate "VC\Tools\MSVC")) {
            return $candidate
        }
    }

    throw "FAIL: could not find a Visual Studio installation with VC tools"
}

function Set-MsvcEnvironment {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (Test-Path $cargoBin) {
        $env:PATH = "$cargoBin;$env:PATH"
    }

    $installRoot = Get-VsInstallRoot
    $vcToolsRoot = Get-LatestDirectory -Path (Join-Path $installRoot "VC\Tools\MSVC")
    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10"
    $sdkLibRoot = Get-LatestDirectory -Path (Join-Path $sdkRoot "Lib")
    $sdkVersion = $sdkLibRoot.Name
    $linkerPath = Join-Path $vcToolsRoot.FullName "bin\Hostx64\x64\link.exe"

    if (-not (Test-Path $linkerPath)) {
        throw "FAIL: MSVC linker not found at $linkerPath"
    }

    $env:PATH = "$(Join-Path $vcToolsRoot.FullName 'bin\Hostx64\x64');$env:PATH"
    $env:LIB = @(
        (Join-Path $vcToolsRoot.FullName "lib\x64"),
        (Join-Path $sdkRoot "Lib\$sdkVersion\ucrt\x64"),
        (Join-Path $sdkRoot "Lib\$sdkVersion\um\x64")
    ) -join ";"
    $env:INCLUDE = @(
        (Join-Path $vcToolsRoot.FullName "include"),
        (Join-Path $sdkRoot "Include\$sdkVersion\ucrt"),
        (Join-Path $sdkRoot "Include\$sdkVersion\shared"),
        (Join-Path $sdkRoot "Include\$sdkVersion\um"),
        (Join-Path $sdkRoot "Include\$sdkVersion\winrt"),
        (Join-Path $sdkRoot "Include\$sdkVersion\cppwinrt")
    ) -join ";"

    $libPathEntries = @((Join-Path $vcToolsRoot.FullName "lib\x64"))
    foreach ($optionalPath in @(
        (Join-Path $sdkRoot "UnionMetadata\$sdkVersion"),
        (Join-Path $sdkRoot "References\$sdkVersion")
    )) {
        if (Test-Path $optionalPath) {
            $libPathEntries += $optionalPath
        }
    }

    $env:LIBPATH = $libPathEntries -join ";"
    $env:VCINSTALLDIR = (Join-Path $installRoot "VC") + "\"
    $env:VCToolsInstallDir = $vcToolsRoot.FullName + "\"
    $env:WindowsSdkDir = $sdkRoot + "\"
    $env:WindowsSDKVersion = $sdkVersion + "\"
    $env:WindowsSDKLibVersion = $sdkVersion + "\"
    $env:UniversalCRTSdkDir = $sdkRoot + "\"
    $env:UCRTVersion = $sdkVersion + "\"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $linkerPath
}

function Invoke-NativeGate {
    param([Parameter(Mandatory = $true)][string]$CargoPath)

    & $CargoPath fmt --check
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & $CargoPath clippy --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    & $CargoPath check --all-targets
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    $hadToken = Test-Path Env:VEX_MODEL_TOKEN
    if ($hadToken) {
        $previousToken = $env:VEX_MODEL_TOKEN
    }
    $env:VEX_MODEL_TOKEN = ""

    try {
        & $CargoPath test --all
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    } finally {
        if ($hadToken) {
            $env:VEX_MODEL_TOKEN = $previousToken
        } else {
            Remove-Item Env:VEX_MODEL_TOKEN -ErrorAction SilentlyContinue
        }
    }

    & $CargoPath test --all-targets
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

if ([string]::IsNullOrWhiteSpace($Target)) {
    Show-Usage
    exit 1
}

Assert-ReleaseMode

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path $cargoBin) {
    $env:PATH = "$cargoBin;$env:PATH"
}

$needsBuild = -not $GateOnly -and -not $PackageOnly
$needsCargo = $true

if ($needsBuild -and $BuildTool -notin @("cargo", "cross")) {
    throw "FAIL: BUILD_TOOL must be 'cargo' or 'cross' (got '$BuildTool')"
}

if ($needsCargo) {
    $cargoPath = Get-RequiredCommand -Name "cargo"
}

$expectedVersion = Get-ExpectedReleaseTag -CargoPath $cargoPath
$Version = Get-NormalizedReleaseTag -Value $Version
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = $expectedVersion
}

if ($Version -notmatch '^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$') {
    throw "FAIL: VERSION must look like v0.1.0 or v0.1.0-alpha.2"
}

if ($Version -ne $expectedVersion) {
    throw "FAIL: VERSION $Version does not match Cargo package tag $expectedVersion"
}

if ($needsBuild) {
    $buildToolPath = if ($BuildTool -eq "cargo") { $cargoPath } else { Get-RequiredCommand -Name $BuildTool }
}

if ($needsBuild -and $Target -like "*windows-msvc") {
    Set-MsvcEnvironment
}

if ($RunGate) {
    Invoke-NativeGate -CargoPath $cargoPath
    if ($GateOnly) {
        return
    }
}

$archiveVersion = $Version.TrimStart("v")
$packageDir = "vex-$archiveVersion-$Target"

if ($Target -like "*windows*") {
    $binaryName = "vex.exe"
    $archiveName = "$packageDir.zip"
} else {
    $binaryName = "vex"
    $archiveName = "$packageDir.tar.gz"
}

$binaryPath = Join-Path "target\$Target\release" $binaryName
$stageDir = Join-Path $OutDir $packageDir
$archivePath = Join-Path $OutDir $archiveName
$checksumPath = "$archivePath.sha256"

if ($needsBuild) {
    & $buildToolPath build --release --target $Target
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

if ($BuildOnly) {
    if (-not (Test-Path $binaryPath)) {
        throw "FAIL: built binary not found at $binaryPath"
    }
    return
}

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

foreach ($path in @($stageDir, $archivePath, $checksumPath)) {
    if (Test-Path $path) {
        Remove-Item $path -Recurse -Force
    }
}

New-Item -ItemType Directory -Path $stageDir -Force | Out-Null

if (-not (Test-Path $binaryPath)) {
    throw "FAIL: built binary not found at $binaryPath"
}

Copy-Item -Path $binaryPath -Destination (Join-Path $stageDir $binaryName)
Copy-Item -Path "README.md" -Destination (Join-Path $stageDir "README.md")
Copy-Item -Path "LICENSE" -Destination (Join-Path $stageDir "LICENSE")

if ($archiveName.EndsWith(".zip")) {
    Push-Location $OutDir
    try {
        Compress-Archive -Path $packageDir -DestinationPath $archiveName -CompressionLevel Optimal
    } finally {
        Pop-Location
    }
} else {
    $tarPath = Get-RequiredCommand -Name "tar.exe"
    & $tarPath -C $OutDir -czf $archivePath $packageDir
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

$hash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $archiveName" | Set-Content -Path $checksumPath -Encoding ascii

Write-Output "archive=$archivePath"
Write-Output "checksum=$checksumPath"
Get-Content $checksumPath
