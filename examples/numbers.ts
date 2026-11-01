// Number formatting must match JavaScript's Number.prototype.toString:
// the shortest decimal string that round-trips back to the same f64.
print(42);
print(12.56636);
print(0.5);
print(0.1);
print(1000000);
print(100000000000000000000);   // 1e20  -> fixed notation
print(1000000000000000000000);  // 1e21  -> exponential notation
print(0.000001);                // 1e-6  -> fixed notation
print(0.0000001);               // 1e-7  -> exponential notation
print(1.0 / 3.0);               // shortest round-tripping form
print(2.0 / 0.0);               // Infinity
print(-0.5);
