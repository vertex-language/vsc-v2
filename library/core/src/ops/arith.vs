//! Arithmetic operator traits (`core/ops`).

package ops

@lang("add")
public trait Add<Rhs = Self> {
    type Output
    func add(self, rhs: Rhs) -> Self.Output
}

@lang("sub")
public trait Sub<Rhs = Self> {
    type Output
    func sub(self, rhs: Rhs) -> Self.Output
}

@lang("mul")
public trait Mul<Rhs = Self> {
    type Output
    func mul(self, rhs: Rhs) -> Self.Output
}

@lang("div")
public trait Div<Rhs = Self> {
    type Output
    func div(self, rhs: Rhs) -> Self.Output
}

@lang("rem")
public trait Rem<Rhs = Self> {
    type Output
    func rem(self, rhs: Rhs) -> Self.Output
}

@lang("neg")
public trait Neg {
    type Output
    func neg(self) -> Self.Output
}

@lang("addAssign")
public trait AddAssign<Rhs = Self> {
    func addAssign(&mut self, rhs: Rhs)
}

@lang("subAssign")
public trait SubAssign<Rhs = Self> {
    func subAssign(&mut self, rhs: Rhs)
}

@lang("mulAssign")
public trait MulAssign<Rhs = Self> {
    func mulAssign(&mut self, rhs: Rhs)
}

@lang("divAssign")
public trait DivAssign<Rhs = Self> {
    func divAssign(&mut self, rhs: Rhs)
}

@lang("remAssign")
public trait RemAssign<Rhs = Self> {
    func remAssign(&mut self, rhs: Rhs)
}
