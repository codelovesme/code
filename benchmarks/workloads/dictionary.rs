// Dictionary: 500 counters under computed names ("k0" … "k499"), each
// incremented 50 times.
use std::collections::HashMap;
fn main() {
    let (keys, passes) = (500, 50);
    let mut counts: HashMap<String, f64> = HashMap::new();
    for k in 0..keys { counts.insert(format!("k{k}"), 0.0); }
    for _ in 0..passes { for k in 0..keys { *counts.get_mut(&format!("k{k}")).unwrap() += 1.0; } }
    let total: f64 = counts.values().sum();
    assert_eq!(total, (keys * passes) as f64);
    println!("{total}");
}
