Indicates that a function does not produce a reply. Callers fire the request without awaiting a response, so results must be `()`.

---

## Syntax

```candid
func (text) -> () oneway
```

## Usage

- Append `oneway` after the result tuple.
- Ensure the result type is `()`.

## Behavior

- No success/failure is reported back to the caller.
- Best suited for best-effort notifications or logging.
