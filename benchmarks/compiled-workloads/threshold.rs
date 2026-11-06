fn main() {
    let n: usize = __SIZE__;
    let mut values: Vec<f64> = Vec::with_capacity(n);
    let mut state = 17.0_f64;
    for _ in 0..n {
        values.push(state);
        state += 17.0;
        if state >= 1024.0 {
            state -= 1024.0;
        }
    }
    let mut result = 0.0_f64;
    for round in 0..16 {
        let mut answer = 0.0_f64;
        for value in &values {
            if *value > 512.0 + round as f64 {
                answer += *value;
            }
        }
        result += answer;
    }
    assert_eq!(result, __EXPECTED__.0);
}
