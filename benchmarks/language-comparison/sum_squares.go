package main

import "fmt"

func main() {
	const n = 100000.0
	const repetitions = 100.0
	const expected = n * (n - 1.0) * (2.0*n - 1.0) / 6.0

	for batch := 0.0; batch != repetitions; batch++ {
		subtotal := 0.0
		for i := 0.0; i != n; i++ {
			subtotal += i * i
		}
		if subtotal != expected {
			fmt.Printf("incorrect sum: %v\n", subtotal)
			panic("benchmark result mismatch")
		}
	}
}
