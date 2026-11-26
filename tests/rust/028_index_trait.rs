use std::ops::{Index, IndexMut};

struct Grid {
    w: usize,
    cells: Vec<i32>,
}

impl Grid {
    fn new(w: usize, h: usize) -> Grid {
        Grid { w, cells: vec![0; w * h] }
    }
}

impl Index<(usize, usize)> for Grid {
    type Output = i32;
    fn index(&self, at: (usize, usize)) -> &i32 {
        &self.cells[at.1 * self.w + at.0]
    }
}

impl IndexMut<(usize, usize)> for Grid {
    fn index_mut(&mut self, at: (usize, usize)) -> &mut i32 {
        &mut self.cells[at.1 * self.w + at.0]
    }
}

struct Polynomial {
    coeffs: Vec<i64>,
}

impl Index<usize> for Polynomial {
    type Output = i64;
    fn index(&self, i: usize) -> &i64 {
        &self.coeffs[i]
    }
}

fn main() {
    let mut g = Grid::new(3, 2);
    g[(0, 0)] = 1;
    g[(2, 1)] = 5;
    g[(1, 1)] += 7;
    g[(1, 1)] *= 2;
    for y in 0..2 {
        for x in 0..3 {
            print!("{} ", g[(x, y)]);
        }
        println!();
    }
    let p = Polynomial { coeffs: vec![3, 0, 2] };
    let x = 4;
    let value = p[0] + p[1] * x + p[2] * x * x;
    println!("p(4) = {value}");
}
