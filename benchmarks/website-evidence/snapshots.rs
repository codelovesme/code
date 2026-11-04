// Same checked matrices as snapshots.code. Default: ordinary Vec<Vec<f64>>
// clones. --cfg cow: Rc plus Rc::make_mut at both container levels.
#[cfg(cow)]
use std::rc::Rc;
const ROWS: usize = 128;
const COLS: usize = 4096;
const VERSIONS: usize = 64;

#[cfg(cow)]
type Matrix = Rc<Vec<Rc<Vec<f64>>>>;
#[cfg(not(cow))]
type Matrix = Vec<Vec<f64>>;

fn main() {
    let initial: Vec<Vec<f64>> = (0..ROWS)
        .map(|r| (0..COLS).map(|c| (r * COLS + c) as f64).collect())
        .collect();
    #[cfg(cow)]
    let mut current: Matrix = Rc::new(initial.into_iter().map(Rc::new).collect());
    #[cfg(not(cow))]
    let mut current: Matrix = initial;
    let mut snapshots = Vec::with_capacity(VERSIONS + 1);
    for s in 0..VERSIONS {
        snapshots.push(current.clone());
        #[cfg(cow)]
        {
            Rc::make_mut(&mut Rc::make_mut(&mut current)[s])[s] += 1.0;
        }
        #[cfg(not(cow))]
        {
            current[s][s] += 1.0;
        }
    }
    snapshots.push(current);
    let n = (ROWS * COLS) as f64;
    let base = n * (n - 1.0) / 2.0;
    for (v, matrix) in snapshots.iter().enumerate() {
        let mut total = 0.0;
        for row in matrix.iter() {
            for x in row.iter() {
                total += x;
            }
        }
        assert_eq!(total, base + v as f64);
    }
    assert_eq!(snapshots.len(), VERSIONS + 1);
}
