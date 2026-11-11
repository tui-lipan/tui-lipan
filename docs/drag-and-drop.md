# Drag and drop with the host terminal

A tui-lipan app can take drops from, and start drags to, the desktop around it when the host
terminal speaks kitty's [drag-and-drop protocol](https://sw.kovidgoyal.net/kitty/dnd-protocol/)
(OSC 72). kitty 0.47 and later do. Everything lives in `tui_lipan::dnd`.

This page covers the app's side: taking drops, and letting the user drag files and text out. For widgets dragged and dropped *within* an app, see
`DragSource` and `DropTarget` in [Events](events.md#drag-and-drop-events).

## Accept drops

The runner asks the host at startup whether it speaks the protocol. Register once the app runs,
and handle what the host sends in the root component's `on_host_drag_and_drop`:

```rust
use tui_lipan::dnd::{
    DataTarget, DropOperation, HostEvent, ProgramCommand, TerminalEvent, URI_LIST,
};

impl Component for App {
    // ...
    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        // Dropped while the host does not speak the protocol.
        ctx.host_drag_and_drop()
            .send(&ProgramCommand::AcceptDrops { mimes: vec![] });
        None
    }

    fn on_host_drag_and_drop(&mut self, event: HostEvent, ctx: &mut Context<Self>) -> Update {
        let host = ctx.host_drag_and_drop();
        match event.event {
            TerminalEvent::DragOver { mimes: Some(mimes), .. } => {
                let wants = mimes.iter().any(|m| m.as_ref() == URI_LIST);
                host.send(&ProgramCommand::DropStatus {
                    operation: if wants { DropOperation::Copy } else { DropOperation::None },
                    mimes: wants.then(|| vec![URI_LIST.into()]),
                });
            }
            TerminalEvent::Drop { mimes, .. } => {
                if let Some(at) = mimes.iter().position(|m| m.as_ref() == URI_LIST) {
                    let index = at as u32 + 1;
                    host.send(&ProgramCommand::RequestData(DataTarget::Mime { index }));
                }
            }
            TerminalEvent::Data { bytes, end, .. } => {
                ctx.state.dropped.extend(bytes);
                if end {
                    host.send(&ProgramCommand::FinishDrop { operation: DropOperation::Copy });
                }
            }
            _ => {}
        }
        Update::full()
    }
}
```

A drop proceeds in this order:

1. `DragOver` arrives as the pointer moves. The first one of a drag lists the MIME types on offer.
   Answer with `DropStatus`; until the app does, the host shows the drop as refused.
2. `Drop` arrives when the user lets go. Only now may the app ask for data.
3. Each `RequestData` is answered with `Data` events, ending with `end` set, or a `DataError`.
4. `FinishDrop` tells the host the app is done. `DropOperation::None` cancels the drop.

`DragLeave` means the drag left without dropping. `parse_uri_list` and `file_uri_path` turn a
`text/uri-list` into local paths.

Commands are written between frames, in the order sent. While the host does not speak the
protocol they are dropped, so registering unconditionally is safe; `HostDragAndDrop::supported`
says which case applies. The runtime withdraws the app's registrations when it exits or hands the
terminal to another program, and registers again when it gets the terminal back.

## Find the widget under a drag

Every event that carries a position (`DragOver`, `Drop`, `DragGesture`) arrives with
`event.targets`: the keyed widgets under the pointer, innermost first, each with its rect and the
pointer in the widget's own cells and pixels. `event.target(|key| ...)` finds the first one a
predicate accepts:

```rust
if let Some(zone) = event.target(|key| key == &Key::from("inbox")) {
    // zone.position is relative to the widget's top-left corner.
}
```

## Files from another machine

A drop may come from another machine, such as a file manager on the desktop while the app runs
over SSH. Send the app's `MachineId` after registering:

```rust
if let Some(id) = host.machine_id() {
    host.send(&ProgramCommand::DropMachineId(id.as_str().into()));
}
```

The host then sets `remote` on the `text/uri-list` data. The app reads each entry's contents with
`RequestData(DataTarget::UriEntry { .. })`, and a directory's children with
`DataTarget::DirEntry`. `Data::kind` says whether an entry is a file, a symbolic link, or a
directory, along with its handle. Release a directory handle with `ReleaseDirectory`.

## Drag out of the app

Widgets say what they offer, and the runtime runs the drag. A file tree takes one call:

```rust
FileTree::new(root).drag_files_out()
```

Dragging an entry to a file manager copies it; dropping it in an editor or a shell gives its path.
The tree's paths must name files on this machine.

Any element can offer something, decided when the drag begins:

```rust
use tui_lipan::dnd::{HostDrag, HostDragSource};

Text::new(&self.summary).host_drag(HostDragSource::new({
    let summary = self.summary.clone();
    move |_start| Some(HostDrag::text(summary.clone()))
}))
```

Widgets of items offer per item, by the item under the pointer: `List::host_drag` with a
`HostDragItems`, `Tree::host_drag` with the row's `TreeEvent`, and `FileTree::host_drag` with the
entry's `FileTreeEvent`. The innermost offer under the pointer wins.

A `HostDrag` carries files, text, or both:

| Builder | Effect |
| --- | --- |
| `HostDrag::files(paths)` | Offer files and directories as a `text/uri-list`, and their paths as text |
| `HostDrag::text(text)` | Offer text |
| `.with_text(text)` | Offer this text instead of the files' paths |
| `.operations(ops)` | What a drop target may do; copy unless set |
| `.label(text)` | The text shown beside the pointer; defaults to the file name, `N files`, or the text |
| `.on_finished(callback)` | Hear the outcome as a `HostDragFinished`: the operation, `None` when canceled |

To let a drop target move files, offer `DropOperations::EITHER` and remove the originals when
`on_finished` reports `DropOperation::Move`.

The runtime registers with the host while something on screen offers a drag, and withdraws once
nothing does. A drag begun over anything else reaches `on_host_drag_and_drop` as a
`DragGesture`, so an app can still run drags of its own. When the host takes over the pointer for
a drag, the press under it ends without a release.

A drop on another machine, such as the desktop of a user running the app over SSH, gets the files'
contents: the runtime reads them off the UI thread, directories whole, and sends them as the host
asks.

### Run a drag yourself

Register with `ProgramCommand::OfferDrags`. When the user starts a drag gesture over the window,
a `DragGesture` arrives; answer with `Offer` and the MIME types on offer, send their data ahead
with `PresentData` and an optional image with `PresentImage`, then `StartDrag`. The host reports
progress with `DragStarted`, `DragAccepted`, `DragDataRequested` (answer with `DragData`),
`DragDropped` and `DragFinished`. Write large data with a `DataStreamWriter` and
`HostDragAndDrop::send_encoded`, so each message stays a whole number of base64 groups.

## Show other programs: the router

An app that runs other programs, such as a multiplexer or an embedded terminal, sits between the
host and those programs. `Router` does the protocol work for it:

- sends a drag to the program under the pointer, in that program's coordinates;
- forwards that program's answers to the host, and only lets the program that took the drop
  read it;
- carries a drag a program starts out through the host, and the host's progress back;
- tells a program on another machine that the files are elsewhere;
- for a program that does not speak the protocol, fetches the dropped paths or text itself and
  returns an `Output::Paste` for the app to paste.

Feed the router host events and program commands, and write what it returns. A
`TerminalScreen` with `set_drag_and_drop_enabled(true)` answers its program's support query,
keeps what the program registered, and queues every other command for
`drain_drag_and_drop_commands`. See
[Terminal widgets](widgets/terminal.md#drag-and-drop-through-osc-72).

## Test it

`TestBackend::set_host_drag_and_drop_supported(true)` stands in for a host that answered.
`TestBackend::host_drag_and_drop` delivers an event to the root component, with targets found in
the rendered tree, and `TestBackend::take_host_drag_and_drop_output` returns what the app sent.
