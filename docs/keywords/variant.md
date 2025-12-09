Tagged unions representing exactly one of several cases. Shorthand omits `: null` for payload-less tags, enabling idiomatic enums.

---

## Type syntax

```candid
variant {}
variant { ok : nat; error : text }
variant { "name with spaces" : nat; "unicode, too: ☃" : bool }
variant { spring; summer; fall; winter }
```

## Textual syntax

```candid
variant { ok = 42 }
variant { "unicode, too: ☃" = true }
variant { fall }
```

---

## Subtypes

- Variants with tags removed.
- Existing tags whose payload types become subtypes.
- Wrap the entire variant in `opt` to add future tags safely in results.

## Supertypes

- Additional tags.
- Payload types widened to supertypes.

---

## Corresponding types/values

### Motoko

```motoko
{ #dot : (); #circle : Float; #rectangle : { width : Float; height : Float } }
```

### Rust

```rust
enum Shape {
    Dot,
    Circle(f64),
    Rectangle { width: f64, height: f64 },
}
```

### JavaScript

```javascript
{ circle: 10n }
{ _2669435721_: "text" } // hashed labels
```
