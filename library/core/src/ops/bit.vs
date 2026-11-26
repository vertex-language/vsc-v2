//! Bitwise and shift operator traits (`core/ops`).

package ops

@lang("not")
public trait Not {
    type Output
    func not(self) -> Self.Output
}

@lang("bitAnd")
public trait BitAnd<Rhs = Self> {
    type Output
    func bitand(self, rhs: Rhs) -> Self.Output
}

@lang("bitOr")
public trait BitOr<Rhs = Self> {
    type Output
    func bitor(self, rhs: Rhs) -> Self.Output
}

@lang("bitXor")
public trait BitXor<Rhs = Self> {
    type Output
    func bitxor(self, rhs: Rhs) -> Self.Output
}

@lang("shl")
public trait Shl<Rhs = Self> {
    type Output
    func shl(self, rhs: Rhs) -> Self.Output
}

@lang("shr")
public trait Shr<Rhs = Self> {
    type Output
    func shr(self, rhs: Rhs) -> Self.Output
}

@lang("bitAndAssign")
public trait BitAndAssign<Rhs = Self> {
    func bitandAssign(&mut self, rhs: Rhs)
}

@lang("bitOrAssign")
public trait BitOrAssign<Rhs = Self> {
    func bitorAssign(&mut self, rhs: Rhs)
}

@lang("bitXorAssign")
public trait BitXorAssign<Rhs = Self> {
    func bitxorAssign(&mut self, rhs: Rhs)
}

@lang("shlAssign")
public trait ShlAssign<Rhs = Self> {
    func shlAssign(&mut self, rhs: Rhs)
}

@lang("shrAssign")
public trait ShrAssign<Rhs = Self> {
    func shrAssign(&mut self, rhs: Rhs)
}
