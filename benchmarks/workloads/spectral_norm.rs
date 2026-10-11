// Spectral norm (Benchmarks Game), N = 1000, ten power iterations — the same
// arithmetic as spectral_norm.c and spectral_norm.code.
const N: usize = 1000;
fn times(v: &[f64], t: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(N);
    for i in 0..N {
        let mut sum = 0.0;
        for j in 0..N {
            let (fi, fj) = (i as f64, j as f64);
            let a = 1.0 / ((fi + fj) * (fi + fj + 1.0) / 2.0 + fi + 1.0 + t * (fj - fi));
            sum += a * v[j];
        }
        out.push(sum);
    }
    out
}
fn main() {
    let mut u = vec![1.0; N];
    let mut v = Vec::new();
    for _ in 0..10 {
        v = times(&times(&u, 0.0), 1.0);
        u = times(&times(&v, 0.0), 1.0);
    }
    let vbv: f64 = u.iter().zip(&v).map(|(a, b)| a * b).sum();
    let vv: f64 = v.iter().map(|b| b * b).sum();
    let r = (vbv / vv).sqrt();
    assert!(r > 1.274224 && r < 1.274225);
    println!("{r:.9}");
}
