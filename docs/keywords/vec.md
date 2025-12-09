The `vec` type represents vectors (sequences, lists, arrays).

A value of type `vec t` contains a sequence of zero or more values of type `t`.

---

## Type syntax

```candid
vec bool
vec nat8
vec vec text
```

## Textual syntax

```candid
vec {}
vec { "john@doe.com"; "john.doe@example.com" };
```

---

## Sub/Supertypes

**Subtypes:**

- Whenever `t` is a subtype of `t'`, then `vec t` is a subtype of `vec t'`.
- `blob` is a subtype of `vec nat8`.

**Supertypes:**

- Whenever `t` is a supertype of `t'`, then `vec t` is a supertype of `vec t'`.
- `blob` is a supertype of `vec nat8`.

---

## Corresponding types/values

### Motoko

```motoko
[T]
// Depending on semantics, can map to BTreeSet/HashSet or maps for record elements.
```

### Rust

```rust
Vec<T>
&[T]
// `vec record { Key; Value }` may translate to BTreeMap or HashMap.
```

### JavaScript

```javascript
["text", "text2", ...]
```
