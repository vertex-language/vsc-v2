//! Canonical target-level calling convention representation (`rustc_abi::canon_abi`).

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CanonAbi {
    /// Target's default C ABI.
    C,
    /// Compiler's native ABI (Rust / Vertex).
    Rust,
    /// Cold native ABI.
    RustCold,
    /// Architecture-specific conventions.
    Arm(ArmCall),
    X86(X86Call),
    Interrupt(InterruptKind),
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InterruptKind {
    Avr,
    AvrNonBlocking,
    Msp430,
    RiscvMachine,
    RiscvSupervisor,
    X86,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum X86Call {
    Fastcall,
    Stdcall,
    SysV64,
    Thiscall,
    Vectorcall,
    Win64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ArmCall {
    Aapcs,
    CCmseNonSecureCall,
    CCmseNonSecureEntry,
}
