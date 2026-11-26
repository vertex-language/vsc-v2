//! Generic hashing support (`core/hash`).

package hash

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { sliceLen } from "core/intrinsics"

/// A trait for hashing an arbitrary stream of bytes.
public trait Hasher {
    func finish(&self) -> uint64
    func write(&mut self, bytes: &[uint8])

    func writeU8(&mut self, i: uint8) {
        let buf: [uint8; 1] = [i]
        self.write(&buf)
    }

    func writeU16(&mut self, i: uint16) {
        let buf: [uint8; 2] = [i as uint8, (i >> 8) as uint8]
        self.write(&buf)
    }

    func writeU32(&mut self, i: uint32) {
        let buf: [uint8; 4] = [
            i as uint8,
            (i >> 8) as uint8,
            (i >> 16) as uint8,
            (i >> 24) as uint8
        ]
        self.write(&buf)
    }

    func writeU64(&mut self, i: uint64) {
        let buf: [uint8; 8] = [
            i as uint8,
            (i >> 8) as uint8,
            (i >> 16) as uint8,
            (i >> 24) as uint8,
            (i >> 32) as uint8,
            (i >> 40) as uint8,
            (i >> 48) as uint8,
            (i >> 56) as uint8
        ]
        self.write(&buf)
    }

    func writeUint(&mut self, i: uint) {
        self.writeU64(i as uint64)
    }

    func writeI8(&mut self, i: int8) { self.writeU8(i as uint8) }
    func writeI16(&mut self, i: int16) { self.writeU16(i as uint16) }
    func writeI32(&mut self, i: int32) { self.writeU32(i as uint32) }
    func writeI64(&mut self, i: int64) { self.writeU64(i as uint64) }
    func writeInt(&mut self, i: int) { self.writeUint(i as uint) }
}

/// A hashable type.
public trait Hash {
    func hash<H: Hasher>(&self, state: &mut H)
}

/// A trait for creating instances of `Hasher`.
public trait BuildHasher {
    type Hasher
    func buildHasher(&self) -> Self.Hasher
}

implement Hash for bool {
    func hash<H: Hasher>(&self, state: &mut H) {
        state.writeU8(if *self { 1 } else { 0 })
    }
}

implement Hash for uint8 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeU8(*self) }
}

implement Hash for uint16 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeU16(*self) }
}

implement Hash for uint32 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeU32(*self) }
}

implement Hash for uint64 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeU64(*self) }
}

implement Hash for uint {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeUint(*self) }
}

implement Hash for int8 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeI8(*self) }
}

implement Hash for int16 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeI16(*self) }
}

implement Hash for int32 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeI32(*self) }
}

implement Hash for int64 {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeI64(*self) }
}

implement Hash for int {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeInt(*self) }
}

implement Hash for char {
    func hash<H: Hasher>(&self, state: &mut H) { state.writeU32(*self as uint32) }
}

/// A simple non-cryptographic 64-bit FNV-1a hasher.
public struct FnvHasher {
    state: uint64
}

implement Copy for FnvHasher {}

implement Clone for FnvHasher {
    func clone(&self) -> FnvHasher { *self }
}

implement Default for FnvHasher {
    func default() -> FnvHasher { FnvHasher.new() }
}

implement FnvHasher {
    public func new() -> FnvHasher {
        FnvHasher { state: 14695981039346656037 }
    }
}

implement Hasher for FnvHasher {
    func finish(&self) -> uint64 {
        self.state
    }

    func write(&mut self, bytes: &[uint8]) {
        var i: uint = 0
        let n = sliceLen(bytes)
        let prime: uint128 = 1099511628211
        while i < n {
            let xored = self.state ^ (bytes[i] as uint64)
            self.state = ((xored as uint128) * prime) as uint64
            i += 1
        }
    }
}
