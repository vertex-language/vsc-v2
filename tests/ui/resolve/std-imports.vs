// Until library/ can be built, only the crate's own packages can be imported.

import (
    "std/fs" //~ ERROR can't find crate for `std`
    "std/io"
    "./missing" //~ ERROR unresolved import `./missing`
)

func main() {}
