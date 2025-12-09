Higher-order reference to a function signature. Allows actors to pass callbacks and captures optional `query`, `composite_query`, or `oneway` annotations.

---

## Syntax

```candid
func (arg : t, ...) -> (res : t, ...) [query|composite_query|oneway]
```

## Textual syntax

```candid
func "aaaaa-aa".method
func "aaaaa-aa"."☃"
```

---

## Subtyping

- Result tuples may be extended or specialized (covariant results).
- Argument tuples may be shortened or widened to supertypes (contravariant arguments).
- Optional values (`opt ...`) can be inserted into either arguments or results.

## Supertypes

- Opposite of the subtyping rules: fewer results, more parameters, or narrower argument types.

---

## Corresponding types/values

### Motoko

```motoko
shared Args -> async Res
// `oneway` drops the `async` return type.
```

### Rust

```rust
candid::IDLValue::Func(Principal, String)
```

### JavaScript

```javascript
[Principal.fromText("aaaaa-aa"), "method"]
```
