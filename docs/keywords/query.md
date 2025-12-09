Marks a function as read-only. Query calls execute on the fast query path, must not mutate state, and may be cached by clients.

---

## Syntax

```candid
func () -> (nat) query
```

## Usage

- Append `query` after the result tuple in a `func` signature.
- Queries can be added/removed following function subtyping rules (e.g., introducing optional results).

## Behavior

- Executes without committing state changes.
- Cannot be composed with `oneway`.
- May return additional values compared to older interfaces as long as function subtyping rules are honored.
