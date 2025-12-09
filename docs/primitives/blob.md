Binary data represented as sequences of bytes (`vec nat8`).

---

## Textual syntax

```candid
blob "hello"
blob "\CA\FF\EE"
```

---

## Sub/Supertypes

**Subtype:**

```candid
vec nat8
```

**Supertype:**

```candid
vec nat8
```

---

## Corresponding types/values

### Motoko types

```motoko
Blob
```

### Rust types

```rust
Vec<u8>
&[u8]
```

### JavaScript values

```javascript
[1, 2, 3]
```
