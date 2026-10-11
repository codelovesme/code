// Binary trees: a complete tree of depth 12 built bottom-up, walked with an
// explicit stack to count its nodes, ten times — the same steps as
// binary_trees.code.
struct Node { kids: Option<(Box<Node>, Box<Node>)> }
fn main() {
    let (depth, reps) = (12, 10u64);
    let leaves = 1usize << depth;
    let mut total = 0u64;
    for _ in 0..reps {
        let mut level: Vec<Box<Node>> = (0..leaves).map(|_| Box::new(Node { kids: None })).collect();
        while level.len() > 1 {
            let mut up = Vec::with_capacity(level.len() / 2);
            let mut it = level.into_iter();
            while let (Some(l), Some(r)) = (it.next(), it.next()) { up.push(Box::new(Node { kids: Some((l, r)) })); }
            level = up;
        }
        let root = level.pop().unwrap();
        let mut stack = vec![&root];
        while let Some(n) = stack.pop() {
            total += 1;
            if let Some((l, r)) = &n.kids { stack.push(l); stack.push(r); }
        }
    }
    assert_eq!(total, reps * (leaves as u64 * 2 - 1));
    println!("{total}");
}
