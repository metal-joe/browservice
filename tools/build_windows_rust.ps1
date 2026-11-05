<#
.SYNOPSIS
Builds the Rust port of Browservice for 64-bit Windows and packages it.

.DESCRIPTION
Windows counterpart of tools/build_windows.py and winbuild/CMakeLists.txt for
the Rust port. Run it on a Windows machine from any directory:

    powershell -NoProfile -ExecutionPolicy Bypass -File tools\build_windows_rust.ps1 -CefTarball C:\path\patched_cef_windows64.tar.bz2

Steps (each one is skipped when its result is already up to date, so the
script can be run again after a failure or a source change):

 1. Set up the MSVC environment (vcvars64.bat through vswhere) unless cl.exe
    is already in PATH.
 2. Get the CEF 156.0.15+gcf671c0+chromium-156.0.8078.80 Windows 64-bit
    distribution: the patched tarball given with -CefTarball (production), or
    the stock standard distribution downloaded from cef-builds.spotifycdn.com
    and checked against its SHA-1 from the CEF build index (test builds with
    -StubPatch only).
 3. Extract it and convert it to the flat layout of the cef-dll-sys crate
    (CEF_PATH).
 4. Clone and bootstrap vcpkg (unless -VcpkgRoot is given) and install
    pango, fontconfig and pkgconf (Rust text renderer) and openssl,
    poco[netssl] and libjpeg-turbo (retrojsvice), triplet x64-windows.
 5. cargo build --release -p browservice-windll: browservice_windll.dll, the
    DLL that exports RunConsoleMain for CEF's bootstrapc.exe.
 6. Build the C++ vice plugin retrojsvice.dll with CMake
    (tools/windows_rust/retrojsvice) and the vcpkg MSBuild integration.
 7. Assemble <OutputDir>\<DistName>: bootstrapc.exe renamed to
    browservice.exe with the manifests of winbuild/ embedded (mt.exe), the
    DLL as browservice.dll, retrojsvice.dll, the vcpkg DLLs, the CEF binary
    and resource files (as COPY_FILES in winbuild/CMakeLists.txt), the MSVC
    redistributable DLLs; set the LPAC ACL needed by the CEF sandbox.
 8. Zip it to <OutputDir>\<DistName>.zip (not with -StubPatch).

Requirements: Visual Studio 2022 (or newer) with "Desktop development with
C++" (MSVC, Windows SDK, C++ CMake tools, which include cmake and ninja),
Rust (rustup, stable, x86_64-pc-windows-msvc toolchain), Git, Python 3 and
the tar.exe of Windows 10/11. Paths must not contain spaces (pkg-config
output is split at spaces).

.PARAMETER CefTarball
Patched CEF tarball for Windows 64-bit (patched_cef_*_windows64.tar.bz2 from
the Browservice releases or built with tools/build_patched_cef.py). Required
for a production build. Without it the stock CEF distribution is downloaded,
which lacks the Browservice Chromium patch symbols and can only be linked with
-StubPatch.

.PARAMETER StubPatch
TEST ONLY: build with the cargo feature stub-patch (no-op stubs of the
Chromium patch functions). The output folder gets the suffix
-stubpatch-TEST-ONLY and no zip is created.

.PARAMETER BuildDir
Directory for downloads, extracted CEF, vcpkg and the retrojsvice build.
Default: <repository>\build_windows_rust.

.PARAMETER VcpkgRoot
Existing vcpkg checkout to use. Default: <BuildDir>\vcpkg (cloned).

.PARAMETER OutputDir
Directory for the distribution folder and zip. Default: <repository>\release.

.PARAMETER DistName
Name of the distribution folder and zip. Default:
browservice-<git describe>-windows64.

.PARAMETER CMakeGenerator
Visual Studio generator for retrojsvice. Default: derived from the Visual
Studio version (e.g. "Visual Studio 17 2022").
#>
[CmdletBinding()]
param(
    [string]$CefTarball = "",
    [switch]$StubPatch,
    [string]$BuildDir = "",
    [string]$VcpkgRoot = "",
    [string]$OutputDir = "",
    [string]$DistName = "",
    [string]$CMakeGenerator = ""
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"
# Invoke-WebRequest is very slow with the progress bar in Windows PowerShell 5.1.
$ProgressPreference = "SilentlyContinue"

# CEF version of the cef crate (cef = "=156.3.0", i.e. 156.3.0+156.0.15).
$CefVersion = "156.0.15+gcf671c0+chromium-156.0.8078.80"
# Stock standard distribution, name and SHA-1 as listed for the windows64
# platform in https://cef-builds.spotifycdn.com/index.json.
$CefStockName = "cef_binary_${CefVersion}_windows64.tar.bz2"
$CefStockSha1 = "dec540c7236c7c98c033f78618c010e3ae322f82"
$CefCdn = "https://cef-builds.spotifycdn.com"
$VcpkgTriplet = "x64-windows"
$VcpkgPackages = @("pango", "fontconfig", "pkgconf", "openssl", "poco[netssl]", "libjpeg-turbo")

# CEF_BINARY_FILES and CEF_RESOURCE_FILES of cmake/cef_variables.cmake in the
# CEF 156 Windows distribution (x86_64), copied like COPY_FILES does in
# winbuild/CMakeLists.txt.
$CefBinaryFiles = @(
    "chrome_elf.dll", "d3dcompiler_47.dll", "libcef.dll", "v8_context_snapshot.bin",
    "vk_swiftshader.dll", "vk_swiftshader_icd.json", "vulkan-1.dll", "dxcompiler.dll"
)
$CefResourceFiles = @("chrome_100_percent.pak", "chrome_200_percent.pak", "resources.pak", "icudtl.dat", "locales")

function Fail([string]$Message) {
    Write-Host "FAIL: $Message" -ForegroundColor Red
    exit 1
}

function Log([string]$Message) {
    Write-Host "--- $Message"
}

# Runs a native program and fails if its exit code is not 0.
function Invoke-Native([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        Fail "Command failed with exit code ${LASTEXITCODE}: $Program $($Arguments -join ' ')"
    }
}

function Require-Command([string]$Name, [string]$Hint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Fail "Required command '$Name' not found in PATH. $Hint"
    }
}

function Reset-Directory([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Path | Out-Null
}

function Get-Sha1([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA1).Hash.ToLowerInvariant()
}

# Writes UTF-8 without byte order mark (serde_json rejects a BOM).
function Write-Utf8File([string]$Path, [string]$Text) {
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

function Read-Stamp([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        return (Get-Content -LiteralPath $Path -Raw).Trim()
    }
    return ""
}

function Get-VsWhere {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $vswhere)) {
        Fail "vswhere.exe not found at '$vswhere'. Install Visual Studio 2022 with 'Desktop development with C++'."
    }
    return $vswhere
}

function Get-VsProperty([string]$Property) {
    $vswhere = Get-VsWhere
    $value = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property $Property
    if ($LASTEXITCODE -ne 0 -or -not $value) {
        Fail "No Visual Studio installation with the MSVC x64 tools found (vswhere)."
    }
    return ($value | Select-Object -First 1).Trim()
}

# Imports the environment of vcvars64.bat into this PowerShell process (the
# cmake crate used by cef-dll-sys needs cl.exe, ninja and the SDK in PATH).
function Import-MsvcEnvironment {
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) {
        Log "MSVC environment already set up (cl.exe found in PATH)"
        return
    }
    $vsPath = Get-VsProperty "installationPath"
    $vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path -LiteralPath $vcvars)) {
        Fail "'$vcvars' not found."
    }
    Log "Importing the MSVC x64 environment from '$vcvars'"
    $helper = Join-Path ([System.IO.Path]::GetTempPath()) ("browservice_vcvars_" + [System.Guid]::NewGuid().ToString("N") + ".cmd")
    Set-Content -LiteralPath $helper -Encoding Ascii -Value @("@call `"$vcvars`" >nul", "@set")
    try {
        # The exit code of vcvars64.bat is not reliable; success is checked
        # below by looking for cl.exe.
        $lines = & cmd.exe /d /c $helper
    } finally {
        Remove-Item -LiteralPath $helper -Force -ErrorAction SilentlyContinue
    }
    foreach ($line in $lines) {
        if ($line -match "^([^=]+)=(.*)$") {
            [System.Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], "Process")
        }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        Fail "cl.exe still not in PATH after running '$vcvars'."
    }
}

function Get-CMakeGenerator {
    $version = $env:VisualStudioVersion
    if (-not $version) {
        $version = Get-VsProperty "installationVersion"
    }
    switch ($version.Split(".")[0]) {
        "16" { return "Visual Studio 16 2019" }
        "17" { return "Visual Studio 17 2022" }
        "18" { return "Visual Studio 18 2026" }
    }
    Fail "Unknown Visual Studio version '$version'; pass -CMakeGenerator."
}

# Runs a native program with stderr discarded and returns its exit code and
# standard output. Windows PowerShell 5.1 turns redirected stderr lines into
# errors, which would stop the script with ErrorActionPreference Stop, so it
# is set to Continue in this function's scope.
function Get-NativeOutput([string]$Program, [string[]]$Arguments) {
    $ErrorActionPreference = "Continue"
    $output = & $Program @Arguments 2>$null
    return @{ Code = $LASTEXITCODE; Output = "$output".Trim() }
}

# Returns @{ Exe = ...; Args = @(...) } for running Python 3.
function Find-Python {
    foreach ($candidate in @(@{ Exe = "python"; Args = @() }, @{ Exe = "py"; Args = @("-3") }, @{ Exe = "python3"; Args = @() })) {
        if (-not (Get-Command $candidate.Exe -ErrorAction SilentlyContinue)) {
            continue
        }
        $result = Get-NativeOutput $candidate.Exe ($candidate.Args + @("-c", "import sys; print(sys.version_info[0])"))
        if ($result.Code -eq 0 -and $result.Output -eq "3") {
            return $candidate
        }
    }
    Fail "Python 3 not found (tried python, py -3, python3)."
}

function Join-Names($Items) {
    return (@($Items | ForEach-Object { $_.Name }) -join ", ")
}

function Assert-NoSpaces([string]$What, [string]$Path) {
    if ($Path.Contains(" ")) {
        Fail "$What '$Path' contains a space; use a path without spaces."
    }
}

# ---------------------------------------------------------------------------

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot "rust\windll\Cargo.toml"))) {
    Fail "'$RepoRoot' does not look like the Browservice repository (rust\windll\Cargo.toml missing)."
}
if (-not $BuildDir) { $BuildDir = Join-Path $RepoRoot "build_windows_rust" }
if (-not $OutputDir) { $OutputDir = Join-Path $RepoRoot "release" }
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$BuildDir = (Resolve-Path $BuildDir).Path
$OutputDir = (Resolve-Path $OutputDir).Path
if (-not $VcpkgRoot) { $VcpkgRoot = Join-Path $BuildDir "vcpkg" }

Assert-NoSpaces "Repository path" $RepoRoot
Assert-NoSpaces "Build directory" $BuildDir
Assert-NoSpaces "vcpkg root" $VcpkgRoot
if ($RepoRoot.Length -gt 40) {
    Write-Warning "The repository path '$RepoRoot' is long; the CEF wrapper build under target\ may hit the 260 character path limit. A short path such as C:\bs is recommended."
}

if (-not $CefTarball -and -not $StubPatch) {
    Fail ("A production build needs the patched CEF (Browservice Chromium patch): pass -CefTarball <patched_cef_*_windows64.tar.bz2>. " +
          "For a test build against the stock CEF $CefVersion, pass -StubPatch.")
}
if ($StubPatch) {
    Write-Warning "-StubPatch: TEST BUILD ONLY. The Chromium patch functions (clipboard, font render mode) are no-op stubs. Do not distribute this build."
}

Log "Checking required tools"
Import-MsvcEnvironment
Require-Command "cargo" "Install Rust with rustup (https://rustup.rs), toolchain stable-x86_64-pc-windows-msvc."
Require-Command "git" "Install Git for Windows."
Require-Command "tar" "tar.exe is part of Windows 10 (1803) and newer."
Require-Command "cmake" "Install the 'C++ CMake tools for Windows' Visual Studio component."
Require-Command "ninja" "Install the 'C++ CMake tools for Windows' Visual Studio component (it contains ninja)."
Require-Command "mt" "mt.exe is part of the Windows SDK (Visual Studio component)."
Require-Command "icacls" ""
$Python = Find-Python
if (-not $CMakeGenerator) { $CMakeGenerator = Get-CMakeGenerator }

# --- CEF -------------------------------------------------------------------

$Downloads = Join-Path $BuildDir "downloads"
New-Item -ItemType Directory -Force -Path $Downloads | Out-Null

if ($CefTarball) {
    if (-not (Test-Path -LiteralPath $CefTarball -PathType Leaf)) {
        Fail "Given CEF tarball '$CefTarball' is not a file."
    }
    $CefArchive = (Resolve-Path -LiteralPath $CefTarball).Path
    Log "Using CEF tarball '$CefArchive'"
} else {
    $CefArchive = Join-Path $Downloads $CefStockName
    if ((Test-Path -LiteralPath $CefArchive) -and ((Get-Sha1 $CefArchive) -eq $CefStockSha1)) {
        Log "Stock CEF distribution already downloaded: '$CefArchive'"
    } else {
        $url = "$CefCdn/" + [System.Uri]::EscapeDataString($CefStockName)
        $partial = "$CefArchive.part"
        Log "Downloading '$url' (about 370 MB)"
        Invoke-WebRequest -Uri $url -OutFile $partial -UseBasicParsing
        $sha1 = Get-Sha1 $partial
        if ($sha1 -ne $CefStockSha1) {
            Remove-Item -LiteralPath $partial -Force
            Fail "SHA-1 of the downloaded CEF distribution is $sha1, expected $CefStockSha1."
        }
        Move-Item -LiteralPath $partial -Destination $CefArchive -Force
    }
}

$CefDist = Join-Path $BuildDir "cef"
$CefStampFile = Join-Path $BuildDir "cef.stamp"
Log "Hashing '$CefArchive'"
$CefStamp = "$(Split-Path -Leaf $CefArchive) $(Get-Sha1 $CefArchive)"
if ((Read-Stamp $CefStampFile) -eq $CefStamp -and (Test-Path -LiteralPath (Join-Path $CefDist "include\cef_version.h"))) {
    Log "CEF distribution already extracted to '$CefDist'"
} else {
    $extractRoot = Join-Path $BuildDir "cef_extract"
    Reset-Directory $extractRoot
    if (Test-Path -LiteralPath $CefDist) { Remove-Item -LiteralPath $CefDist -Recurse -Force }
    Remove-Item -LiteralPath $CefStampFile -Force -ErrorAction SilentlyContinue
    Log "Extracting '$CefArchive' to '$extractRoot'"
    Invoke-Native "tar" @("-xf", $CefArchive, "-C", $extractRoot)
    $dirs = @(Get-ChildItem -LiteralPath $extractRoot -Directory)
    if ($dirs.Count -ne 1) {
        Fail "Could not identify the CEF binary distribution directory in '$extractRoot', candidates: $(Join-Names $dirs)"
    }
    Move-Item -LiteralPath $dirs[0].FullName -Destination $CefDist
    Remove-Item -LiteralPath $extractRoot -Recurse -Force
    Set-Content -LiteralPath $CefStampFile -Value $CefStamp
}

foreach ($required in @("include\cef_version.h", "Release\libcef.dll", "Release\libcef.lib", "Release\bootstrapc.exe", "Resources\locales", "CMakeLists.txt", "cmake", "libcef_dll")) {
    if (-not (Test-Path -LiteralPath (Join-Path $CefDist $required))) {
        Fail "'$CefDist' is not a complete CEF Windows binary distribution ('$required' missing)."
    }
}
$versionLine = Select-String -LiteralPath (Join-Path $CefDist "include\cef_version.h") -Pattern '^#define CEF_VERSION "(.*)"$' | Select-Object -First 1
if (-not $versionLine) { Fail "Could not read CEF_VERSION from '$CefDist\include\cef_version.h'." }
$DistCefVersion = $versionLine.Matches[0].Groups[1].Value
if ($DistCefVersion -ne $CefVersion) {
    Fail "The CEF distribution has version '$DistCefVersion', but the cef crate needs '$CefVersion'."
}
Log "CEF version $DistCefVersion"

# Flat layout expected by cef-dll-sys (like download-cef's extraction and
# tools/prepare_cef_for_rust.sh): Release\* and Resources\* in one directory
# together with include\, cmake\, libcef_dll\, CMakeLists.txt and archive.json.
$CefRust = Join-Path $BuildDir "cef_rust"
$CefRustStampFile = Join-Path $BuildDir "cef_rust.stamp"
if ((Read-Stamp $CefRustStampFile) -eq $CefStamp -and (Test-Path -LiteralPath (Join-Path $CefRust "archive.json"))) {
    Log "CEF already prepared for cef-dll-sys in '$CefRust'"
} else {
    Log "Preparing CEF for cef-dll-sys in '$CefRust'"
    Remove-Item -LiteralPath $CefRustStampFile -Force -ErrorAction SilentlyContinue
    Reset-Directory $CefRust
    Copy-Item -Path (Join-Path $CefDist "Release\*") -Destination $CefRust -Recurse
    Copy-Item -Path (Join-Path $CefDist "Resources\*") -Destination $CefRust -Recurse
    foreach ($item in @("include", "cmake", "libcef_dll", "CMakeLists.txt", "CREDITS.html")) {
        $source = Join-Path $CefDist $item
        if (Test-Path -LiteralPath $source) {
            Copy-Item -LiteralPath $source -Destination (Join-Path $CefRust $item) -Recurse
        }
    }
    $archiveJson = "{`n  `"type`": `"standard`",`n  `"name`": `"cef_binary_${DistCefVersion}_windows64.tar.bz2`",`n  `"sha1`": `"`"`n}`n"
    Write-Utf8File (Join-Path $CefRust "archive.json") $archiveJson
    Set-Content -LiteralPath $CefRustStampFile -Value $CefStamp
}

# --- vcpkg -----------------------------------------------------------------

if (-not (Test-Path -LiteralPath (Join-Path $VcpkgRoot ".vcpkg-root"))) {
    Log "Cloning vcpkg to '$VcpkgRoot'"
    Invoke-Native "git" @("clone", "https://github.com/microsoft/vcpkg", $VcpkgRoot)
}
$Vcpkg = Join-Path $VcpkgRoot "vcpkg.exe"
if (-not (Test-Path -LiteralPath $Vcpkg)) {
    Log "Bootstrapping vcpkg"
    Invoke-Native (Join-Path $VcpkgRoot "bootstrap-vcpkg.bat") @("-disableMetrics")
}
$VcpkgInstallArgs = @("install", "--triplet", $VcpkgTriplet) + $VcpkgPackages
Log "Installing vcpkg packages $($VcpkgPackages -join ' ') ($VcpkgTriplet)"
Push-Location $VcpkgRoot
try {
    Invoke-Native $Vcpkg $VcpkgInstallArgs
} finally {
    Pop-Location
}
$VcpkgInstalled = Join-Path $VcpkgRoot "installed\$VcpkgTriplet"
$PkgConf = Get-ChildItem -LiteralPath (Join-Path $VcpkgInstalled "tools") -Recurse -Filter "pkgconf.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $PkgConf) {
    Fail "pkgconf.exe not found under '$VcpkgInstalled\tools' (vcpkg package pkgconf)."
}

# --- Rust ------------------------------------------------------------------

$env:CEF_PATH = $CefRust
$env:PKG_CONFIG = $PkgConf.FullName
$env:PKG_CONFIG_PATH = Join-Path $VcpkgInstalled "lib\pkgconfig"
$CargoArgs = @("build", "--release", "-p", "browservice-windll")
if ($StubPatch) { $CargoArgs += @("--features", "stub-patch") }
Log "Building the Rust DLL: cargo $($CargoArgs -join ' ') (CEF_PATH=$env:CEF_PATH, PKG_CONFIG=$env:PKG_CONFIG)"
Push-Location $RepoRoot
try {
    Invoke-Native "cargo" $CargoArgs
} finally {
    Pop-Location
}
$TargetDir = $env:CARGO_TARGET_DIR
if (-not $TargetDir) { $TargetDir = Join-Path $RepoRoot "target" }
$RustDll = Join-Path $TargetDir "release\browservice_windll.dll"
if (-not (Test-Path -LiteralPath $RustDll)) {
    Fail "Built DLL '$RustDll' not found."
}

# --- retrojsvice -----------------------------------------------------------

$RetrojsviceDir = Join-Path $RepoRoot "viceplugins\retrojsvice"
$RetrojsviceBuild = Join-Path $BuildDir "retrojsvice"
$GenHtmlCpp = Join-Path $RetrojsviceBuild "gen\html.cpp"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $GenHtmlCpp) | Out-Null
Log "Generating '$GenHtmlCpp'"
# gen_html_cpp.py prints the file; its stdout is redirected to the file
# inside Python so that PowerShell does not re-encode the output.
$genCode = "import runpy, sys; sys.stdout = open(sys.argv[1], 'w', newline='\n'); runpy.run_path('gen_html_cpp.py', run_name='__main__'); sys.stdout.close()"
Push-Location $RetrojsviceDir
try {
    Invoke-Native $Python.Exe ($Python.Args + @("-c", $genCode, $GenHtmlCpp))
} finally {
    Pop-Location
}

$RetrojsviceCMakeBuild = Join-Path $RetrojsviceBuild "build"
New-Item -ItemType Directory -Force -Path $RetrojsviceCMakeBuild | Out-Null
# vcpkg MSBuild integration for the generated projects, as in tools/build_windows.py.
foreach ($pair in @(@("Directory.Build.props", "vcpkg.props"), @("Directory.Build.targets", "vcpkg.targets"))) {
    $ref = Join-Path $VcpkgRoot "scripts\buildsystems\msbuild\$($pair[1])"
    $xml = "<?xml version=`"1.0`" encoding=`"utf-8`"?>`n<Project>`n  <Import Project=`"$ref`" />`n</Project>`n"
    Write-Utf8File (Join-Path $RetrojsviceCMakeBuild $pair[0]) $xml
}
Log "Configuring retrojsvice with CMake ($CMakeGenerator)"
Invoke-Native "cmake" @(
    "-S", (Join-Path $RepoRoot "tools\windows_rust\retrojsvice"),
    "-B", $RetrojsviceCMakeBuild,
    "-G", $CMakeGenerator, "-A", "x64",
    "-DRETROJSVICE_DIR=$RetrojsviceDir",
    "-DRETROJSVICE_GEN_HTML_CPP=$GenHtmlCpp"
)
Log "Building retrojsvice"
Invoke-Native "cmake" @("--build", $RetrojsviceCMakeBuild, "--config", "Release", "--target", "retrojsvice")
$RetrojsviceDll = Join-Path $RetrojsviceCMakeBuild "Release\retrojsvice.dll"
if (-not (Test-Path -LiteralPath $RetrojsviceDll)) {
    Fail "Built plugin '$RetrojsviceDll' not found."
}

# --- MSVC redistributable DLLs -----------------------------------------------

if (-not $env:VCToolsRedistDir) {
    Fail "VCToolsRedistDir is not set; run the script from a plain PowerShell (it sets up the MSVC environment) or a Developer PowerShell."
}
$redistArch = Join-Path $env:VCToolsRedistDir "x64"
$crtDirs = @(Get-ChildItem -LiteralPath $redistArch -Directory | Where-Object { $_.Name -like "Microsoft.VC*.CRT" })
if ($crtDirs.Count -ne 1) {
    Fail "Could not identify the MSVC redistributable DLL directory in '$redistArch', candidates: $(Join-Names $crtDirs)"
}
$Redists = @()
foreach ($name in @("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll")) {
    $path = Join-Path $crtDirs[0].FullName $name
    if (-not (Test-Path -LiteralPath $path)) { Fail "'$path' not found." }
    $Redists += $path
}

# --- Distribution ----------------------------------------------------------

if (-not $DistName) {
    $describe = Get-NativeOutput "git" @("-C", $RepoRoot, "describe", "--tags", "--always", "--dirty")
    $label = $describe.Output
    if ($describe.Code -ne 0 -or -not $label) { $label = "unknown" }
    $DistName = "browservice-$label-windows64"
}
if ($StubPatch) { $DistName = "$DistName-stubpatch-TEST-ONLY" }
$DistDir = Join-Path $OutputDir $DistName
Log "Assembling the distribution in '$DistDir'"
Reset-Directory $DistDir

$cefRelease = Join-Path $CefDist "Release"
$cefResources = Join-Path $CefDist "Resources"

# bootstrapc.exe loads <exe name>.dll from its directory and calls RunConsoleMain.
$exe = Join-Path $DistDir "browservice.exe"
Copy-Item -LiteralPath (Join-Path $cefRelease "bootstrapc.exe") -Destination $exe
Copy-Item -LiteralPath $RustDll -Destination (Join-Path $DistDir "browservice.dll")
# ADD_WINDOWS_MANIFEST in winbuild/CMakeLists.txt: embed the manifests.
Invoke-Native "mt" @(
    "-nologo",
    "-manifest", (Join-Path $RepoRoot "winbuild\browservice.exe.manifest"), (Join-Path $RepoRoot "winbuild\compatibility.manifest"),
    "-outputresource:$exe;#1"
)

Copy-Item -LiteralPath $RetrojsviceDll -Destination $DistDir
# Runtime DLLs of pango/fontconfig (Rust DLL) and Poco/OpenSSL/libjpeg-turbo
# (retrojsvice) and their dependencies.
Copy-Item -Path (Join-Path $VcpkgInstalled "bin\*.dll") -Destination $DistDir

foreach ($name in $CefBinaryFiles) {
    $source = Join-Path $cefRelease $name
    if (-not (Test-Path -LiteralPath $source)) { Fail "CEF binary file '$source' not found." }
    Copy-Item -LiteralPath $source -Destination $DistDir -Recurse
}
foreach ($name in $CefResourceFiles) {
    $source = Join-Path $cefResources $name
    if (-not (Test-Path -LiteralPath $source)) { Fail "CEF resource file '$source' not found." }
    Copy-Item -LiteralPath $source -Destination $DistDir -Recurse
}
foreach ($path in $Redists) {
    Copy-Item -LiteralPath $path -Destination $DistDir
}

# SET_LPAC_ACLS of the CEF CMake macros: the sandboxed GPU/utility processes
# (LPAC) need read access to the application directory. Not stored in the zip.
Log "Setting LPAC ACLs on '$DistDir'"
Invoke-Native "icacls" @($DistDir, "/grant", "*S-1-15-2-2:(OI)(CI)(RX)", "/Q")

if ($StubPatch) {
    Log "Test build (stub-patch) ready in '$DistDir'; no zip is created for it"
    Log "Success"
    exit 0
}

$Zip = Join-Path $OutputDir "$DistName.zip"
Log "Creating '$Zip'"
if (Test-Path -LiteralPath $Zip) { Remove-Item -LiteralPath $Zip -Force }
Compress-Archive -LiteralPath $DistDir -DestinationPath $Zip
Log "Browservice built successfully: '$Zip'"
Log "Success"
