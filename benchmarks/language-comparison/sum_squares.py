N = 100_000.0
REPETITIONS = 100.0
EXPECTED = N * (N - 1.0) * (2.0 * N - 1.0) / 6.0

batch = 0.0
while batch != REPETITIONS:
    i = 0.0
    subtotal = 0.0
    while i != N:
        subtotal = subtotal + i * i
        i = i + 1.0
    if subtotal != EXPECTED:
        raise SystemExit(f"incorrect sum: {subtotal}")
    batch = batch + 1.0
