package main

struct Grid {
    w: uint
    cells: Vec<int32>
}

implement Grid {
    func new(w: uint, h: uint) -> Grid {
        Grid { w, cells: vec[0; w * h] }
    }
}

implement Index<(uint, uint)> for Grid {
    type Output = int32
    func index(&self, at: (uint, uint)) -> &int32 {
        &self.cells[at.1 * self.w + at.0]
    }
}

implement IndexMut<(uint, uint)> for Grid {
    func indexMut(&mut self, at: (uint, uint)) -> &mut int32 {
        &mut self.cells[at.1 * self.w + at.0]
    }
}

struct Polynomial {
    coeffs: Vec<int64>
}

implement Index<uint> for Polynomial {
    type Output = int64
    func index(&self, i: uint) -> &int64 {
        &self.coeffs[i]
    }
}

func main() {
    var g = Grid.new(3, 2)
    g[(0, 0)] = 1
    g[(2, 1)] = 5
    g[(1, 1)] += 7
    g[(1, 1)] *= 2
    for y in 0..2 {
        for x in 0..3 {
            print("{} ", g[(x, y)])
        }
        println()
    }
    let p = Polynomial { coeffs: vec[3, 0, 2] }
    let x = 4
    let value = p[0] + p[1] * x + p[2] * x * x
    println("p(4) = {value}")
}
