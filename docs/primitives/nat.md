Unbounded non-negative integers encoded using LEB128 (small numbers stay compact).

---

## Textual syntax

```candid
1234
1_000_000
0xDEAD_BEEF
```

---

## Sub/Supertypes

- Supertype: `int`

---

## Corresponding types/values

### Motoko types

```motoko
Nat
```

### Rust types

```rust
candid::Nat
```
```rust
u128 // common host type
```

### JavaScript values

```javascript
BigInt(42)
42n
```
