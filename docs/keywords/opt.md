Optional container that stores any value of type `t` plus the distinguished `null`. Crucial for evolving interfaces where fields or results may be absent.

---

## Syntax

```candid
opt t
```

## Textual syntax

```candid
null
opt true
opt opt "text"
```

---

## Subtyping

- If `t` <: `t'` then `opt t` <: `opt t'`.
- Every `t` (unless `t` is `null`, `opt ...`, or `reserved`) is a subtype of `opt t`.
- `null` is a subtype of any `opt t`.

## Supertypes

- Mirror of the above: if `t` :> `t'`, then `opt t` :> `opt t'`.

---

## Corresponding types/values

### Motoko

```motoko
?T
```

### Rust

```rust
Option<T>
```

### JavaScript

```javascript
[]        // null
[8]       // opt 8
[["text"]] // opt opt "text"
```
