Marks a method as a composite query: it may issue cross-canister query calls yet still must avoid state mutation.

---

## Syntax

```candid
func () -> (Foo) composite_query
```

## Usage

- Append after the result tuple, similar to `query`.
- Enables query-time fan-out to other canisters while remaining read-only.

## Behavior

- Runs on the replicated execution path that allows other query calls.
- Cannot mutate local state and cannot be mixed with `oneway`.
