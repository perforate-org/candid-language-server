Unsigned 64-bit integers (`0..2^64-1`).

---

## Textual syntax

Same as `nat`; annotate to disambiguate.

```candid
42 : nat64
```

---

## Corresponding types/values

### Motoko types

```motoko
Nat64
Word64
```

### Rust types

```rust
u64
```

### JavaScript values

```javascript
BigInt(42)
42n
```
