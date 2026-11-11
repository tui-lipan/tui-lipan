//! A widget that offers a drag out of the app: the runtime registers with the host while one is on
//! screen, and a drag begun on a file tree row offers that row's file.

use std::cell::RefCell;
use std::rc::Rc;

use tui_lipan::TestBackend;
use tui_lipan::dnd::{
    CommandDecoder, DragPosition, DropOperation, HostDrag, HostDragFinished, HostDragSource,
    HostEvent, OscScanner, ProgramCommand, TaggedEvent, TerminalEvent, URI_LIST,
};
use tui_lipan::prelude::*;

#[derive(Default)]
struct State {
    heard: Vec<HostEvent>,
    offers: bool,
}

struct Files {
    finished: Rc<RefCell<Vec<DropOperation>>>,
}

impl Component for Files {
    type Message = bool;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State {
            offers: true,
            ..State::default()
        }
    }

    fn update(&mut self, offers: bool, ctx: &mut Context<Self>) -> Update {
        ctx.state.offers = offers;
        Update::full()
    }

    fn on_host_drag_and_drop(&mut self, event: HostEvent, ctx: &mut Context<Self>) -> Update {
        if event.event == TerminalEvent::DragLeave {
            // Stands in for an app that offers drags of its own.
            ctx.host_drag_and_drop()
                .send(&ProgramCommand::OfferDrags { machine_id: None });
        }
        ctx.state.heard.push(event);
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let finished = Rc::clone(&self.finished);
        let tree = FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [
                        FileTreeEntry::file("first.txt"),
                        FileTreeEntry::file("second.txt"),
                    ],
                ),
            ]))
            .change_source(FileTreeChangeSource::Provided(Vec::new()))
            .scrollbar(false);
        let tree = if ctx.state.offers {
            tree.host_drag(move |entry| {
                let finished = Rc::clone(&finished);
                Some(
                    HostDrag::files([entry.path.as_ref()]).on_finished(Callback::new(
                        move |done: HostDragFinished| finished.borrow_mut().push(done.operation),
                    )),
                )
            })
        } else {
            tree
        };
        let notes = Text::new("notes").key("notes");
        let notes = if ctx.state.offers {
            notes.host_drag(HostDragSource::new(|start| {
                Some(HostDrag::text(format!("dragged from column {}", start.x)))
            }))
        } else {
            notes
        };
        VStack::new().child(tree).child(notes).into()
    }
}

fn backend() -> (TestBackend<Files>, Rc<RefCell<Vec<DropOperation>>>) {
    let finished = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Files {
        finished: Rc::clone(&finished),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 8,
    });
    backend.set_host_drag_and_drop_supported(true);
    backend.render();
    (backend, finished)
}

fn sent(backend: &mut TestBackend<Files>) -> Vec<ProgramCommand> {
    let bytes = backend.take_host_drag_and_drop_output();
    let mut decoder = CommandDecoder::default();
    OscScanner::default()
        .scan(&bytes)
        .into_iter()
        .flat_map(|(_, body)| decoder.push_body(&body))
        .map(|tagged| tagged.command)
        .collect()
}

fn host(backend: &mut TestBackend<Files>, event: TerminalEvent) {
    backend
        .host_drag_and_drop(TaggedEvent {
            client_id: None,
            event,
        })
        .unwrap();
}

/// Where `label` is drawn.
fn cell_of(backend: &mut TestBackend<Files>, label: &str) -> DragPosition {
    let lines = backend.capture_frame().to_fixed_grid_lines();
    let (row, line) = lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .unwrap_or_else(|| panic!("{label} on screen:\n{}", lines.join("\n")));
    DragPosition {
        cell_x: line[..line.find(label).unwrap()].chars().count() as u32 + 1,
        cell_y: row as u32,
        pixel_x: 0,
        pixel_y: 0,
    }
}

fn presented(sent: &[ProgramCommand], index: u32) -> Vec<u8> {
    sent.iter()
        .filter_map(|command| match command {
            ProgramCommand::PresentData {
                mime_index, bytes, ..
            } if *mime_index == index => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn a_drag_begun_on_a_row_offers_that_rows_file() {
    let (mut backend, finished) = backend();
    assert!(
        matches!(sent(&mut backend)[..], [ProgramCommand::OfferDrags { .. }]),
        "registered while something on screen offers a drag"
    );
    let position = cell_of(&mut backend, "second.txt");
    host(&mut backend, TerminalEvent::DragGesture { position });
    let offer = sent(&mut backend);
    let ProgramCommand::Offer { mimes, .. } = &offer[0] else {
        panic!("{offer:?}");
    };
    assert_eq!(mimes[0].as_ref(), URI_LIST);
    assert_eq!(presented(&offer, 0), b"file:///remote/repo/second.txt\r\n");
    assert_eq!(presented(&offer, 1), b"/remote/repo/second.txt");
    assert_eq!(offer.last(), Some(&ProgramCommand::StartDrag));
    assert!(
        backend.state().heard.is_empty(),
        "the app does not hear the runtime's drag"
    );

    host(&mut backend, TerminalEvent::DragStarted);
    host(&mut backend, TerminalEvent::DragAccepted { mime_index: 0 });
    host(
        &mut backend,
        TerminalEvent::DragFinished { canceled: false },
    );
    backend.pump().unwrap();
    assert_eq!(*finished.borrow(), [DropOperation::Copy]);
}

#[test]
fn any_element_can_offer_and_the_rest_of_the_screen_stays_the_apps() {
    let (mut backend, _) = backend();
    sent(&mut backend);
    let position = cell_of(&mut backend, "notes");
    host(&mut backend, TerminalEvent::DragGesture { position });
    let offer = sent(&mut backend);
    assert!(
        String::from_utf8(presented(&offer, 0))
            .unwrap()
            .starts_with("dragged from column "),
        "{offer:?}"
    );

    let empty = DragPosition {
        cell_x: 30,
        cell_y: 7,
        pixel_x: 0,
        pixel_y: 0,
    };
    host(&mut backend, TerminalEvent::DragGesture { position: empty });
    assert!(sent(&mut backend).is_empty());
    assert_eq!(
        backend.state().heard.len(),
        1,
        "a gesture over nothing is the app's"
    );
}

#[test]
fn the_registration_is_withdrawn_once_nothing_offers_a_drag() {
    let (mut backend, _) = backend();
    sent(&mut backend);
    backend.dispatch(false).unwrap();
    assert_eq!(sent(&mut backend), [ProgramCommand::StopOfferingDrags]);
    backend.dispatch(true).unwrap();
    assert!(matches!(
        sent(&mut backend)[..],
        [ProgramCommand::OfferDrags { .. }]
    ));
}

#[test]
fn an_app_that_offers_drags_itself_keeps_its_registration() {
    let (mut backend, _) = backend();
    sent(&mut backend);
    host(&mut backend, TerminalEvent::DragLeave);
    assert!(matches!(
        sent(&mut backend)[..],
        [ProgramCommand::OfferDrags { .. }]
    ));
    backend.dispatch(false).unwrap();
    assert!(
        sent(&mut backend).is_empty(),
        "the runtime does not withdraw what the app registered"
    );
}
