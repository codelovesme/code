#[derive(Clone, Copy)]
struct Record {
    x: f64,
    y: f64,
}
fn main() {
    let n: usize = __SIZE__;
    let mut values: Vec<Record> = Vec::with_capacity(n);
    let mut state = 17.0_f64;
    for _ in 0..n {
        values.push(Record {
            x: state,
            y: 1024.0 - state,
        });
        state += 17.0;
        if state >= 1024.0 {
            state -= 1024.0;
        }
    }
    let mut result = 0.0_f64;
    for round in 0..16 {
        let mut answer = 3000000.0_f64;
        for value in &values {
            let dx = value.x - round as f64 * 17.0;
            let dy = value.y - round as f64 * 11.0;
            let distance = dx * dx + dy * dy;
            if distance < answer {
                answer = distance;
            }
        }
        result += answer;
    }
    assert_eq!(result, __EXPECTED__.0);
}
