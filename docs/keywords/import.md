`import` lets you pull definitions from another Candid file so you can reuse shared types or services.

```candid
import "./shared.did";
```

Paths can be relative or absolute. Each import must end with a semicolon and can only appear at the top level of a Candid file before other declarations.
