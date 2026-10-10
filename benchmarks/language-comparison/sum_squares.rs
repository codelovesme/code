fn main() {
    let n = 100_000.0_f64;
    let repetitions = 100.0_f64;
    let expected = n * (n - 1.0) * (2.0 * n - 1.0) / 6.0;

    let mut batch = 0.0_f64;
    while batch != repetitions {
        let mut i = 0.0_f64;
        let mut subtotal = 0.0_f64;
        while i != n {
            subtotal += i * i;
            i += 1.0;
        }
        if subtotal != expected {
            eprintln!("incorrect sum: {subtotal}");
            std::process::exit(1);
        }
        batch += 1.0;
    }
}
