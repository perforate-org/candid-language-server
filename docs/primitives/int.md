Unbounded signed integers encoded using SLEB128.

---

## Textual syntax

```candid
1234
-1234
+1_000_000
0xDEAD_BEEF
-0xDEAD_BEEF
```

---

## Sub/Supertypes

**Subtype:**

```candid
nat
```

---

## Corresponding types/values

### Motoko types

```motoko
Int
```

### Rust types

```rust
candid::Int
```

```rust
i128 // common host type
```

### JavaScript values

```javascript
BigInt(-42)
-42n
```
