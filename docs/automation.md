# Automation

tui-lipan automation drives the same mounted session as terminal, test, recording,
capture, and web frontends. A transport parses requests into typed operations. It
does not inspect or mutate the realized node tree itself.

## Runtime boundary

`RuntimeCore<C>` owns the component instance, component state, scoped message
queue, commands, nested components, and reconciliation data.

`SessionEngine<C>` owns lifecycle, viewport, semantic projection, duplicate-ID
validation, and committed generations around that core. The operation executor
in the same module owns selector resolution, semantic-action validation, input
ordering, clock policy, and fixed-point draining.

`AppRunner`, `TestBackend`, and `AutomationSession` implement the executor's
host interface. They retain their renderer and platform-specific interaction
state. Terminal setup, raw mode, native input workers, PTY presentation caches,
and inline transcript presentation stay outside the engine.

## Identity

`AutomationId` and `Key` have different jobs.

```rust
Button::new("Save").automation_id("save")
```

An automation ID:

- is optional, but is the only refactor-stable automation identity;
- must be unique across the complete realized semantic tree, including overlays;
- is supplied by the app author and is never random;
- remains stable across renders and independent runs when the app supplies the
  same value.

Duplicate automation IDs fail the commit in every build. `Key` remains a
sibling-scoped reconciliation identity and a focus-restoration mechanism. It is
not an automation ID.

The script spelling `#save` refers to `AutomationId("save")`. Scripts that used
`#name` for a reconciliation key must add `.automation_id("name")`. This is a
breaking change.

## Semantic tree

Each committed generation has one hierarchical `SemanticTree`. It starts with a
synthetic root. Application content appears first, followed by overlay roots in
their effective stacking order. A realized widget appears exactly once, even
when an overlay portal is reachable from more than one internal traversal.

An overlay portal moves its content out of the place the app declared it and
leaves an empty placeholder behind. The role, accessible name, and automation ID
the widget author put on that declaration belong to the content, so the
projection carries them onto the hoisted root and omits the placeholder: a
`Modal` is one `Dialog` node with the dialog's own bounds and actions, not an
invisible `Dialog` beside a `Group` that holds the real thing. A declaration on
the content itself always wins over the one on the portal.

Each semantic node reports:

- an optional automation ID;
- a stable `SemanticRole`;
- accessible name and a safe value;
- focused, enabled, selected, expanded, and checked state when applicable;
- layout bounds and effective bounds after ancestor and viewport clipping;
- `in_view`, which means the clipped bounds have positive area;
- `actionable`, which means the node is enabled and supports at least one
  semantic action;
- supported semantic actions and child nodes.

Internal `NodeId` values are absent from public snapshots and wire protocols.
`exists` means a matching node is present in the semantic tree. It is a query
condition, not a stored visibility flag.

Values carry sensitivity metadata. Masked values and values explicitly marked
sensitive are always redacted in semantic snapshots and the Markdown or JSON
derived from them, including those formats returned over the control protocol.
Pixel artifacts are different: PNGs, recordings, and visual baselines capture
what the user can see. They can contain sensitive text when the rendered widget
does not mask it, so treat those artifacts as sensitive data.

## Selectors

Selectors are explicit variants. They do not fall through to another strategy.

```rust
Selector::id("save")
Selector::role(SemanticRole::Button).name("Save")
Selector::text_contains("Connection failed")
Selector::point(42, 17)
```

ID selectors carry the app-author stability guarantee. Role, accessible-name,
and text selectors are intended for discovery and readable tests. Point
selectors are explicitly tied to layout and viewport size.

An action requires exactly one match. No match returns `NoMatch`; more than one
returns `AmbiguousMatch`. Discovery APIs may return all matches.

## Operation sequencing

`AutomationStep` is a typed, non-exhaustive operation:

```rust
AutomationStep::click(Selector::id("save"))
AutomationStep::resize(120, 40)
AutomationStep::advance(Duration::from_millis(250))
AutomationStep::wait_for(condition, timeout)
AutomationStep::checkpoint("editor")
```

One UI thread performs every operation. A sequence stops at the first error and
does not roll back successful earlier steps. Composite input preserves terminal
event order. A click sends move, button down, and button up in that order.

After each operation, the engine drains ready framework effects, command
messages, component messages, and timers due at the current logical time to a
fixed point. It then commits one coherent generation containing viewport,
semantic tree, interaction state, and rendered frame. Results include the
zero-based step index and resulting generation.

Text scripts compile into these operations. Socket and future MCP transports
use versioned wire request and response types. They never serialize Rust public
API types as their protocol.

Text selectors use `#id`, `@role`, `@role=Accessible name`, or
`text~substring`. Persistent scripts also accept `resize:120x40`, `drain`,
`checkpoint:name`, and `wait-for:PREDICATE,SELECTOR,MILLISECONDS`. Every
condition below has a script spelling; the ones carrying a value write it into
the predicate, as `value=ready`, `text=Connected`, `count=3`, or
`selected=false`. `docs/testing.md` has the full table.

## Clocks and idle

Clock mode is fixed when a session starts:

```rust
ClockMode::Realtime
ClockMode::Controlled
```

Each session has its own clock. Realtime sessions follow elapsed wall time.
Controlled sessions move only through `advance`; sleeping or waiting for an
external command never advances logical application time. Framework timestamps,
animation ticks, overlay expiry, and framework timers use the session clock.
Wall-clock operation deadlines remain separate.

Controlled timers are not eligible for the global realtime timer worker. An
explicit advance runs timers that became due in submission order and continues
draining immediate message and timer chains to a bounded fixed point.

`drain_ready()` processes work runnable at the current logical time.
`wait_for_idle(timeout, quiet_window)` waits for bounded realtime quiescence of
tracked session work. `IdleReport` includes queued messages, due and future
timers, dirty state, and tracked commands.

The runtime tracks commands it creates and messages sent through their
`CommandLink`s. It cannot prove that arbitrary application threads holding an
ordinary `Link` will never send again. Idle reports state this boundary instead
of treating an empty queue as proof of permanent idleness.

## Wait conditions

The stable condition set is deliberately small:

- exists or missing;
- in view;
- focused;
- enabled or disabled;
- selected;
- value equals;
- text contains;
- count.

A controlled wait does not advance logical time. It may spend wall time waiting
for an external command while the logical clock remains frozen.

Timeout errors include the requested condition, elapsed wall and logical time,
last match set, activity or idle report, the last semantic snapshot, and an
optional diagnostic checkpoint.

## Checkpoints

A checkpoint commits evidence for a generation. It does not mean "write a PNG".
Session options configure a closed set of sinks:

- semantic JSON;
- Markdown;
- PNG;
- recording marker or frame;
- baseline comparison.

Semantic JSON and Markdown preserve sensitivity metadata and omit sensitive
values. Pixel-based sinks do not perform redaction; they faithfully capture the
rendered frame.

The engine encodes an artifact in memory before writing it. File sinks write a
same-directory temporary file and atomically rename it into place. A checkpoint
with several files is individually atomic per file, not transactionally atomic
as a group.

Requesting a format absent from the current build returns `UnsupportedFormat`
before creating a file. The runtime never writes Markdown under a `.png` or
`.json` name.

`resize` preserves the mounted application and changes its current layout.
Fresh-state viewport matrices require a component or session factory and replay
the scenario independently for each viewport.

## Lifecycle

Initialization and unmount belong to the session. The root and nested components
unmount exactly once on normal exit, errors, dropped headless sessions, and
transport shutdown. Shutdown rejects late messages and cancels or detaches
tracked pending work without allowing it to mutate the closed session.

## Control protocol

Unix sockets are one transport for the automation request model. The protocol is
versioned and bounded. Negotiation exchanges protocol version and capabilities.
Each request has an ID and deadline; another request can cancel it by ID.
Replies carry structured errors and capture bytes rather than arbitrary
server-side output paths.

Implementations enforce request and response size limits before execution.
Malformed requests, unsupported capabilities, expired deadlines, and
cancellation fail without mutating the session.

TCP is out of scope until tui-lipan has an authentication and threat model. A
Windows named-pipe transport may later carry the same wire request types.

Set both variables to run an app as a persistent off-screen server:

```sh
TUI_LIPAN_AUTOMATION_HEADLESS=1 \
TUI_LIPAN_CONTROL=/tmp/my-app.sock \
cargo run --example todo
```

`TUI_LIPAN_AUTOMATION_VIEWPORT=120x40` changes the initial viewport.
`TUI_LIPAN_AUTOMATION_ARTIFACTS=path` changes where `checkpoint:` writes
semantic Markdown. `snapshot markdown` and `snapshot json` return the semantic
tree; `snapshot png` returns the rendered frame.

## Errors

Automation errors distinguish at least:

- invalid selector or script;
- no match;
- ambiguous match;
- duplicate automation ID;
- target not actionable or not in view;
- wait timeout;
- unsupported operation or artifact format;
- protocol limit, deadline, or cancellation;
- closed session;
- component/runtime and I/O failures.

Errors name the failed operation and preserve its index. They do not imply that
earlier operations were undone.
