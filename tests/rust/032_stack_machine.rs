#[derive(Debug)]
enum Op {
    Push(i64),
    Add,
    Mul,
    Sub,
    Dup,
    Swap,
    Jz(usize),
    Jmp(usize),
    Print,
}

fn run(prog: &[Op]) -> Vec<i64> {
    let mut stack: Vec<i64> = Vec::new();
    let mut out = Vec::new();
    let mut pc = 0;
    let mut steps = 0;
    while pc < prog.len() && steps < 1000 {
        steps += 1;
        match &prog[pc] {
            Op::Push(v) => stack.push(*v),
            Op::Add => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                stack.push(a + b);
            }
            Op::Mul => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                stack.push(a * b);
            }
            Op::Sub => {
                let b = stack.pop().unwrap_or(0);
                let a = stack.pop().unwrap_or(0);
                stack.push(a - b);
            }
            Op::Dup => {
                let top = stack[stack.len() - 1];
                stack.push(top);
            }
            Op::Swap => {
                let n = stack.len();
                let t = stack[n - 1];
                stack[n - 1] = stack[n - 2];
                stack[n - 2] = t;
            }
            Op::Jz(target) => {
                if stack.pop().unwrap_or(0) == 0 {
                    pc = *target;
                    continue;
                }
            }
            Op::Jmp(target) => {
                pc = *target;
                continue;
            }
            Op::Print => out.push(stack[stack.len() - 1]),
        }
        pc += 1;
    }
    out
}

fn main() {
    // Count down from 5, printing each value.
    let prog = [
        Op::Push(5),
        Op::Print,
        Op::Push(1),
        Op::Sub,
        Op::Dup,
        Op::Jz(7),
        Op::Jmp(1),
        Op::Push(6),
        Op::Push(7),
        Op::Mul,
        Op::Push(2),
        Op::Add,
        Op::Push(100),
        Op::Swap,
        Op::Sub,
        Op::Print,
    ];
    println!("{:?}", run(&prog));
    println!("{:?}", prog[5]);
}
