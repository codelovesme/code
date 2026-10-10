const n = 100_000.0;
const repetitions = 100.0;
const expected = n * (n - 1.0) * (2.0 * n - 1.0) / 6.0;

let batch = 0.0;
while (batch !== repetitions) {
  let i = 0.0;
  let subtotal = 0.0;
  while (i !== n) {
    subtotal = subtotal + i * i;
    i = i + 1.0;
  }
  if (subtotal !== expected) {
    throw new Error(`incorrect sum: ${subtotal}`);
  }
  batch = batch + 1.0;
}
