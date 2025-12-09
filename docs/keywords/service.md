Declares a full service (actor) interface so that an entire canister signature can be referenced or passed around.

---

## Type syntax

```candid
service {
  add : (nat) -> ();
  subtract : (nat) -> ();
  get : () -> (int) query;
  subscribe : (func (int) -> ()) -> ();
}
```

## Textual syntax

```candid
service "w7x7r-cok77-xa"
service "zwigo-aiaaa-aaaaa-qaa3a-cai"
service "aaaaa-aa"
```

---

## Subtypes

- Services with methods added.
- Existing methods specialized to subtypes (following function subtyping rules).

## Supertypes

- Methods removed.
- Method signatures widened to supertypes.

---

## Corresponding types/values

### Motoko

```motoko
actor {
  add : shared Nat -> async ()
  subtract : shared Nat -> async ();
  get : shared query () -> async Int;
  subscribe : shared (shared Int -> async ()) -> async ();
}
```

### Rust

```rust
candid::IDLValue::Service(Principal)
```

### JavaScript

```javascript
Principal.fromText("aaaaa-aa")
```
