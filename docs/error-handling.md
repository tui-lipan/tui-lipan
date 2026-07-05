# Error handling

## Policy: panics vs `Error` variants

tui-lipan uses two mechanisms for failures, chosen by **who can diagnose the problem**:

| Mechanism | When to use | Examples |
|-----------|------------|----------|
| `panic!` / `unreachable!` | Internal invariant violated - a framework bug | Arena index out of bounds, type-erased downcast on a value the framework itself stored, `NodeTree` epoch mismatch |
| `Error` variant | Condition an **app author** might encounter or need to handle | Props type mismatch during component expansion, message routing to wrong component type, font file not found, I/O failure |

### Guidelines

- **Never panic on user-supplied data.** If a user provides a bad font path, wrong props type, or invalid configuration, return a structured `Error`.
- **Panic on impossible states.** If the framework's own invariants are violated (e.g. a `ScopeId` points to a freed slot, or `AnyProps` stores a type different from what it was created with), panic with a descriptive message. These are bugs, not runtime errors.
- **Prefer `Result` propagation over `unwrap()`.** When a fallible operation has a graceful alternative (e.g. rendering empty text when a font fails to load), use the fallback instead of panicking.
- **Use `Error::ComponentExpansion`** for failures during the component mount/expansion pipeline that prevent a component from being created. The framework falls back to an empty component rather than crashing the entire application.

## Error variants

See `src/lib.rs` for the full `Error` enum. Key variants:

- `Io` - I/O errors (terminal, file reads)
- `SyntaxThemeLoad` - syntax highlighting theme loading failures
- `MessageTypeMismatch` - message routed to a component expecting a different message type
- `PropsTypeMismatch` - properties type mismatch during component mounting
- `ComponentExpansion` - broader component expansion failure (wraps mount failures)
