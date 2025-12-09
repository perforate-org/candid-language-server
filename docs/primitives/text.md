Human-readable Unicode text (any sequence of Unicode code points, excluding surrogate parts).

---

## Textual syntax

```candid
""
"Hello"
```

### Unicode escapes

```candid
"\u{2603}" // ☃
"\u{221E}" // ∞
```

### Raw bytes (must be utf8)

```candid
"\E2\98\83" // ☃
```

---

## Corresponding types/values

### Motoko type

```motoko
Text
```

### Rust types

```rust
String
&str
```

### JavaScript values

```javascript
"String"
```
