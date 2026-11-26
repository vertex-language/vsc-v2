//! Indexing traits (`core/ops`).

package ops

@lang("index")
public trait Index<Idx> {
    type Output
    func index(&self, index: Idx) -> &Self.Output
}

@lang("indexMut")
public trait IndexMut<Idx>: Index<Idx> {
    func indexMut(&mut self, index: Idx) -> &mut <Self as Index<Idx>>.Output
}
