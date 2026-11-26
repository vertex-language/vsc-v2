use crate::ParseSess;
use crate::config::{
    CrateType, OptLevel, Options, OutFileName, OutputType, TargetInfo, default_configuration_with_panic,
};
use crate::session::Session;
use vsc_span::Symbol;
use vsc_target::spec::PanicStrategy;

#[test]
fn target_info_converts_to_target() {
    let info = TargetInfo::from_triple("aarch64-apple-darwin").unwrap();
    let target = info.to_target().unwrap();
    assert_eq!(target.target_triple, "arm64-apple-macosx");
    assert_eq!(target.pointer_width, 64);
}

#[test]
fn panic_cfg_tracks_strategy() {
    let info = TargetInfo::from_triple("x86_64-unknown-linux-gnu").unwrap();
    let cfg = default_configuration_with_panic(&info, false, false, PanicStrategy::Abort);
    assert!(cfg.contains(Symbol::intern("panic"), Some(Symbol::intern("abort"))));
}

#[test]
fn session_builds_output_filenames() {
    let mut opts = Options::default();
    opts.crate_types.push(CrateType::Executable);
    opts.cg.opt_level = OptLevel::parse("2").unwrap();
    opts.out_file = Some(OutFileName::Real("target/out/hello".into()));
    let sess = Session::new(opts, ParseSess::with_stderr(vsc_errors::ColorConfig::Auto)).unwrap();
    let out = sess.output_filenames("hello", None);
    assert_eq!(out.exe_path(&sess.target), std::path::Path::new("target/out/hello"));
    assert_eq!(
        out.path(OutputType::Object),
        OutFileName::Real("target/out/hello".into())
    );
}
