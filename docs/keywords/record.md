Collection of labeled values. Field order does not matter, textual labels are hashed, and omitted labels become sequential numbers (tuple syntax).

---

## Syntax

```candid
record {
  first_name : text;
  score : nat;
}

record { text; nat; opt bool }
```

## Textual syntax

```candid
record { first_name = "John"; score = 42 }
record { "a"; "tuple"; null }
```

---

## Subtypes

- Records with additional fields.
- Fields whose types change to subtypes.
- Optional fields removed (commonly replaced with `opt empty`).

## Supertypes

- Records with fewer fields.
- Fields widened to supertypes.
- Additional optional fields to allow backward-compatible argument growth.

---

## Corresponding types/values

### Motoko

```motoko
(type) ({ first_name : Text; score : Nat })
// Sequential labels map to tuples (T1, T2, ...).
```

### Rust

```rust
#[derive(CandidType, Deserialize)]
struct Foo {
    #[serde(rename = "first_name")]
    first_name: String,
    score: i64,
}
```

### JavaScript

```javascript
{ "first name": "Candid", age: 42 }
// Tuple-style records become arrays such as ["Candid", 42].
```
