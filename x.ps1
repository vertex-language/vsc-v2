# The build entry point on Windows. See src/bootstrap/src/main.rs.
Set-Location $PSScriptRoot
cargo run -q -p bootstrap -- @args
exit $LASTEXITCODE
