use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tui_lipan::prelude::*;

struct ProvidedFileTreeApp;

#[derive(Default)]
struct State {
    listings: Vec<FileTreeDirectoryListing>,
    pending: HashSet<Arc<str>>,
}

enum Msg {
    ListDirectory(Arc<str>),
    DirectoryListed(FileTreeDirectoryListing),
}

impl Component for ProvidedFileTreeApp {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ListDirectory(path) => {
                if !ctx.state.pending.insert(path.clone()) {
                    return Update::none();
                }
                Update::with_command(ctx.link().command(move |link| {
                    // Replace this delay and fixture data with an SSH/session-client round trip.
                    std::thread::sleep(Duration::from_millis(250));
                    link.send(Msg::DirectoryListed(list_directory(path)));
                }))
            }
            Msg::DirectoryListed(listing) => {
                ctx.state.pending.remove(&listing.path);
                ctx.state
                    .listings
                    .retain(|current| current.path != listing.path);
                ctx.state.listings.push(listing);
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tree = FileTree::new("/remote/project")
            .entry_source(FileTreeEntrySource::provided(ctx.state.listings.clone()))
            .on_entry_request(
                ctx.link()
                    .callback(|request: FileTreeEntryRequest| Msg::ListDirectory(request.path)),
            );

        Frame::new()
            .title("Remote project")
            .status("Expand directories to request their listings")
            .border(true)
            .child(tree)
            .into()
    }
}

fn list_directory(path: Arc<str>) -> FileTreeDirectoryListing {
    let modified = GitFileStatus::new(None, Some(GitChangeState::Modified));
    let entries = match path.as_ref() {
        "/remote/project" => vec![
            FileTreeEntry::directory("src"),
            FileTreeEntry::directory("target").ignored(true),
            FileTreeEntry::file("README.md").git_status(modified),
        ],
        "/remote/project/src" => vec![
            FileTreeEntry::file("lib.rs").git_status(modified),
            FileTreeEntry::file("main.rs"),
        ],
        "/remote/project/target" => vec![FileTreeEntry::file("debug.log").ignored(true)],
        _ => return FileTreeDirectoryListing::error(path, "directory not found"),
    };
    FileTreeDirectoryListing::new(path, entries)
}

fn main() -> Result<()> {
    App::new()
        .title("Provided FileTree")
        .mount(ProvidedFileTreeApp)
        .run()
}
