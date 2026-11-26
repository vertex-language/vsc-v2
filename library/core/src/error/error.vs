//! Interfaces for working with errors (`core/error`).

package error

import { Debug, Display } from "core/fmt"

/// `Error` is a trait representing the basic expectations for error values.
public trait Error: Debug + Display {}
