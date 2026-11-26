#!/bin/sh
# The build entry point, as in rustc. See src/bootstrap/src/main.rs.
set -e
cd "$(dirname "$0")"
exec cargo run -q -p bootstrap -- "$@"
