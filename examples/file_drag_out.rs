//! Drag files out of a FileTree to other applications, in a terminal that supports drag and drop
//! (kitty 0.47 or later). Pass a directory to browse; the current one is the default.
use std::path::PathBuf;

use tui_lipan::dnd::{DropOperation, HostDrag, HostDragFinished};
use tui_lipan::prelude::*;

struct Files {
    root: PathBuf,
}

#[derive(Default)]
struct State {
    status: String,
}

impl Component for Files {
    type Message = HostDragFinished;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State {
            status: "Drag a file into another application.".into(),
        }
    }

    fn update(&mut self, finished: HostDragFinished, ctx: &mut Context<Self>) -> Update {
        let names: Vec<_> = finished
            .files
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        ctx.state.status = match finished.operation {
            DropOperation::None => format!("Canceled {}", names.join(", ")),
            operation => format!("{operation:?} of {}", names.join(", ")),
        };
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let finished = ctx.link().callback(|done: HostDragFinished| done);
        let supported = if ctx.host_drag_and_drop().supported() {
            ""
        } else {
            " (this terminal does not support drag and drop)"
        };
        VStack::new()
            .child(
                FileTree::new(self.root.to_string_lossy().into_owned()).host_drag(move |entry| {
                    Some(HostDrag::files([entry.path.as_ref()]).on_finished(finished.clone()))
                }),
            )
            .child(Text::new(format!("{}{supported}", ctx.state.status)).height(Length::Px(1)))
            .into()
    }
}

fn main() -> tui_lipan::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    App::new()
        .title("Drag files out")
        .mount(Files { root })
        .run()
}
