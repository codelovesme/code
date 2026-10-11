// String building: 20,000 numbers joined with commas, one at a time.
use std::fmt::Write;
fn main() {
    let mut s = String::new();
    for i in 0..20000 { write!(s, "{i},").unwrap(); }
    assert_eq!(s.chars().count(), 108890);
    println!("{}", s.len());
}
