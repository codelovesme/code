#[derive(Clone, Copy)]
struct Record {
    x: f64,
    y: f64,
    z: f64,
}
fn main() {
    let n: usize = __SIZE__;
    let mut values: Vec<Record> = Vec::with_capacity(n);
    let mut state = 17.0_f64;
    for _ in 0..n {
        values.push(Record {
            x: state,
            y: 1024.0 - state,
            z: state * 2.0,
        });
        state += 17.0;
        if state >= 1024.0 {
            state -= 1024.0;
        }
    }
    let mut result = 0.0_f64;
    for round in 0..16 {
        let mut answer = 0.0_f64;
        for value in &values {
            answer = answer + value.x * (round as f64 + 1.0) + value.y * 3.0 - value.z * 2.0;
        }
        result += answer;
    }
    assert_eq!(result, __EXPECTED__.0);
}
