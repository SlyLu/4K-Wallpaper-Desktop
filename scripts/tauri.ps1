$ErrorActionPreference = 'Stop'

# Rustup is not always added to PATH in non-interactive Windows hosts.
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH = "$cargoBin;$env:PATH"

# Prefer the ABI-compatible user-level MinGW64 toolchain when MSVC Build Tools are unavailable.
$portableToolchains = Join-Path $env:LOCALAPPDATA 'CodexToolchains'
$mingwBin = Join-Path $portableToolchains 'msys2\msys64\mingw64\bin'
if (Test-Path -LiteralPath $mingwBin) {
    $env:PATH = "$mingwBin;$env:PATH"
}

& pnpm exec tauri @args
exit $LASTEXITCODE
