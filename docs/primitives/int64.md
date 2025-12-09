Signed 64-bit integers (`-2^63..2^63-1`).

---

## Textual syntax

Same as `int`; annotate to disambiguate.

```candid
-42 : int64
```

---

## Corresponding types/values

### Motoko types

```motoko
Int64
```

### Rust types

```rust
i64
```

### JavaScript values

```javascript
BigInt(-42)
-42n
```
