int main(void) {
    const double n = 100000.0;
    const double repetitions = 100.0;
    const double expected = n * (n - 1.0) * (2.0 * n - 1.0) / 6.0;

    double batch = 0.0;
    while (batch != repetitions) {
        double i = 0.0;
        double subtotal = 0.0;
        while (i != n) {
            subtotal = subtotal + i * i;
            i = i + 1.0;
        }
        if (subtotal != expected) {
            return 1;
        }
        batch = batch + 1.0;
    }
    return 0;
}
