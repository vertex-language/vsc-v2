use std::sync::Arc;

use vsc_span::{BytePos, FileName, SourceMap, Span};

use crate::{Applicability, DiagCtxt, ErrCode, HumanEmitter, SharedBuffer};

fn setup(src: &str) -> (Arc<SourceMap>, BytePos, DiagCtxt, SharedBuffer) {
    let sm = Arc::new(SourceMap::new());
    let file = sm.new_source_file(FileName::Custom("test.vs".into()), src.to_string());
    let buf = SharedBuffer::new();
    let dcx = DiagCtxt::new(Box::new(HumanEmitter::new(Box::new(buf.clone()), false).with_source_map(sm.clone())));
    (sm, file.start_pos, dcx, buf)
}

fn sp(base: BytePos, lo: u32, hi: u32) -> Span {
    Span::new(BytePos(base.0 + lo), BytePos(base.0 + hi))
}

#[test]
fn single_label() {
    let (_sm, base, dcx, buf) = setup("func main() {\n    let a = b\n}\n");
    dcx.struct_span_err(sp(base, 26, 27), "cannot find value `b` in this scope")
        .with_code(ErrCode(425))
        .with_span_label(sp(base, 26, 27), "not found in this scope")
        .emit();
    assert_eq!(
        buf.contents(),
        "error[E0425]: cannot find value `b` in this scope\n \
         --> <test.vs>:2:13\n  \
         |\n\
         2 |     let a = b\n  \
         |             ^ not found in this scope\n\n"
    );
    assert_eq!(dcx.err_count(), 1);
}

#[test]
fn two_labels_on_one_line() {
    let (_sm, base, dcx, buf) = setup("foo(a, b)\n");
    dcx.struct_span_err(sp(base, 7, 8), "mismatched")
        .with_span_label(sp(base, 7, 8), "second")
        .with_span_label(sp(base, 0, 3), "first")
        .emit();
    assert_eq!(
        buf.contents(),
        "error: mismatched\n \
         --> <test.vs>:1:8\n  \
         |\n\
         1 | foo(a, b)\n  \
         | ---    ^ second\n  \
         | |\n  \
         | first\n\n"
    );
}

#[test]
fn notes_and_suggestions() {
    let (_sm, base, dcx, buf) = setup("let x = a::b\n");
    dcx.struct_span_err(sp(base, 9, 11), "paths use `.`, not `::`")
        .with_note("Vertex separates path segments with `.`")
        .with_span_suggestion(sp(base, 9, 11), "use `.`", ".", Applicability::MachineApplicable)
        .emit();
    dcx.print_error_count();
    assert_eq!(
        buf.contents(),
        "error: paths use `.`, not `::`\n \
         --> <test.vs>:1:10\n  \
         |\n\
         1 | let x = a::b\n  \
         |          ^^\n  \
         |\n  \
         = note: Vertex separates path segments with `.`\n\
         help: use `.`\n  \
         |\n\
         1 | let x = a.b\n  \
         |          ~\n\n\
         error: aborting due to 1 previous error\n\n"
    );
}

#[test]
fn multiline_span_and_gap() {
    let (_sm, base, dcx, buf) = setup("a\nb\nc\nd\ne\n");
    dcx.struct_span_warn(sp(base, 0, 1), "first and last")
        .with_span_label(sp(base, 8, 9), "here")
        .emit();
    assert_eq!(
        buf.contents(),
        "warning: first and last\n \
         --> <test.vs>:1:1\n  \
         |\n\
         1 | a\n  \
         | ^\n\
         ...\n\
         5 | e\n  \
         | - here\n\n"
    );
    assert_eq!(dcx.warn_count(), 1);
    assert_eq!(dcx.err_count(), 0);
}
