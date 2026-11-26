//! Static archive (`ar`) creation for `staticlib` / `rlib` crate types (`rustc_codegen_ssa::back::archive`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use vsc_session::Session;

/// Builds a static archive at `output` containing every object file in `objects`.
pub fn create_static_library(sess: &Session, objects: &[PathBuf], output: &Path) {
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = fs::create_dir_all(parent)
    {
        sess.dcx().fatal(format!("failed to create directory `{}`: {e}", parent.display()));
    }

    // Remove pre-existing archive so deleted symbols do not linger.
    let _ = fs::remove_file(output);

    let ar = find_ar();
    let mut cmd = Command::new(&ar);
    cmd.arg("rcs").arg(output).args(objects);

    match cmd.output() {
        Ok(res) if res.status.success() => {}
        Ok(res) => {
            let stderr = String::from_utf8_lossy(&res.stderr);
            sess.dcx().fatal(format!(
                "archiver `{ar}` failed with status {}: {stderr}",
                res.status
            ));
        }
        Err(err) => {
            sess.dcx().fatal(format!("failed to invoke archiver `{ar}`: {err}"));
        }
    }
}

fn find_ar() -> String {
    if let Ok(ar) = std::env::var("AR")
        && !ar.is_empty()
    {
        return ar;
    }
    "ar".to_string()
}
