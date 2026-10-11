// Points: 1,000 { x, y, vx, vy } moved 200 steps, bouncing off a box.
#[derive(Clone, Copy)]
struct Point { x: f64, y: f64, vx: f64, vy: f64 }
fn main() {
    let mut pts: Vec<Point> = (0..1000).map(|i| Point { x: i as f64, y: 1000.0 - i as f64, vx: 3.0, vy: -2.0 }).collect();
    for _ in 0..200 {
        for p in pts.iter_mut() {
            p.x += p.vx; p.y += p.vy;
            if p.x < 0.0 || p.x > 1000.0 { p.vx = -p.vx; }
            if p.y < 0.0 || p.y > 1000.0 { p.vy = -p.vy; }
        }
    }
    let sum: f64 = pts.iter().map(|p| p.x + p.y).sum();
    println!("{sum}");
    assert_eq!(sum, 1001398.0);
}
