fn main() {
    let n: usize = __SIZE__;
    let mut prime = vec![true; n + 1];
    prime[0] = false;
    prime[1] = false;
    let mut p = 2;
    while p * p <= n {
        if prime[p] {
            let mut multiple = p * p;
            while multiple <= n {
                prime[multiple] = false;
                multiple += p;
            }
        }
        p += 1;
    }
    let mut count = 0_u64;
    let mut sum = 0_u64;
    for i in 2..=n {
        if prime[i] {
            count += 1;
            sum += i as u64;
        }
    }
    assert_eq!(count, __COUNT__);
    assert_eq!(sum, __SUM__);
}
