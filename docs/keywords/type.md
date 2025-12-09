Introduces a named alias for any Candid type. Naming a type improves readability, documentation, and import/export ergonomics without altering the underlying type hash.

---

## Syntax

```candid
type Address = record {
  street : text;
  city : text;
};
```

## Usage

```candid
type Shape = variant {
  dot;
  circle : float64;
};
```

## Notes

- Labels and field order inside the aliased type follow the normal rules for records/variants/etc.
- Subtyping behavior is identical to the aliased structure; the `type` keyword does not relax constraints.
- Named types can be imported, exported, and referenced like built-in ones.
