fn main() {
    let n: usize = __SIZE__;
    let mut a = vec![0.0_f64; n * n];
    let mut b = vec![0.0_f64; n * n];
    let mut c = vec![0.0_f64; n * n];
    for i in 0..n {
        for j in 0..n {
            a[i * n + j] = (i + j + 1) as f64 / n as f64;
            b[i * n + j] = (i * 2 + j + 1) as f64 / n as f64;
        }
    }
    for i in 0..n {
        for k in 0..n {
            let x = a[i * n + k];
            for j in 0..n {
                c[i * n + j] += x * b[k * n + j];
            }
        }
    }
    let sum_k = (n * (n - 1)) as f64 / 2.0;
    let sum_k2 = (n * (n - 1) * (2 * n - 1)) as f64 / 6.0;
    for i in 0..n {
        for j in 0..n {
            let u = (i + 1) as f64;
            let v = (j + 1) as f64;
            let expected =
                (2.0 * sum_k2 + (2.0 * u + v) * sum_k + n as f64 * u * v) / (n * n) as f64;
            let difference = c[i * n + j] - expected;
            assert!(difference > -1e-8 && difference < 1e-8);
        }
    }
}
