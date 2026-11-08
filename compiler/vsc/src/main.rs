//! The `vsc` binary (rustc's `compiler/rustc`).

fn main() -> std::process::ExitCode {
    vsc_driver_impl::main()
}
