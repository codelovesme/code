fn main() {
    let n: usize = __SIZE__;
    let mut values = vec![0.0_f64; n];
    let mut scratch = vec![0.0_f64; n];
    let mut seed = 17;
    for i in 0..n {
        values[i] = seed as f64;
        scratch[i] = 0.0;
        seed += 8191;
        if seed >= n {
            seed -= n;
        }
    }
    let mut width = 1;
    while width < n {
        let mut left = 0;
        while left < n {
            let mid = (left + width).min(n);
            let right = (left + 2 * width).min(n);
            let mut a = left;
            let mut b = mid;
            let mut out = left;
            while a < mid && b < right {
                if values[a] <= values[b] {
                    scratch[out] = values[a];
                    a += 1;
                } else {
                    scratch[out] = values[b];
                    b += 1;
                }
                out += 1;
            }
            while a < mid {
                scratch[out] = values[a];
                a += 1;
                out += 1;
            }
            while b < right {
                scratch[out] = values[b];
                b += 1;
                out += 1;
            }
            left += 2 * width;
        }
        std::mem::swap(&mut values, &mut scratch);
        width *= 2;
    }
    for i in 0..n {
        assert_eq!(values[i], i as f64);
    }
}
