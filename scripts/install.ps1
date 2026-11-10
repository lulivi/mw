# Downloads the latest mw release for Windows into the current directory:
#   irm https://raw.githubusercontent.com/lulivi/mw/main/scripts/install.ps1 | iex
& {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue' # the progress bar makes Invoke-WebRequest very slow
  $archive = 'mw-x86_64-pc-windows-gnu.zip' # also runs on Windows on ARM, under emulation
  $url = "https://github.com/lulivi/mw/releases/latest/download/$archive"
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
  New-Item -ItemType Directory $tmp | Out-Null
  try {
    Write-Host "Downloading $archive"
    Invoke-WebRequest $url -OutFile "$tmp/$archive" -UseBasicParsing
    Invoke-WebRequest "$url.sha256" -OutFile "$tmp/$archive.sha256" -UseBasicParsing
    $expected = (Get-Content "$tmp/$archive.sha256").Split(' ')[0]
    $actual = (Get-FileHash "$tmp/$archive" -Algorithm SHA256).Hash
    if ($actual -ne $expected) { throw "checksum mismatch for $archive" }
    Expand-Archive "$tmp/$archive" $tmp
    Move-Item -Force "$tmp/mw.exe" (Join-Path $PWD 'mw.exe')
    Write-Host "Installed $(Join-Path $PWD 'mw.exe'). Run .\mw.exe --help to get started."
  } finally {
    Remove-Item -Recurse -Force $tmp
  }
}
