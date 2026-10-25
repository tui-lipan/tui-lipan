//! Per-screen OSC 22 stacks. Only known names reach host output.
use std::io::{self, Write};

use crate::style::PointerShape;

const STACK_LIMIT: usize = 16;

#[derive(Debug, Default)]
pub(super) struct PointerState {
    primary: Vec<PointerShape>,
    alternate: Vec<PointerShape>,
    pub alternate_active: bool,
    pub response: Option<Vec<u8>>,
}

impl PointerState {
    pub fn current(&self) -> Option<PointerShape> {
        self.stack(self.alternate_active).last().copied()
    }

    fn stack(&self, alternate: bool) -> &[PointerShape] {
        if alternate {
            &self.alternate
        } else {
            &self.primary
        }
    }

    fn stack_mut(&mut self) -> &mut Vec<PointerShape> {
        if self.alternate_active {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    pub fn dispatch(&mut self, value: &[u8]) {
        let Ok(value) = std::str::from_utf8(value) else {
            return;
        };
        match value.as_bytes().first() {
            Some(b'?') => self.query(&value[1..]),
            Some(b'<') => {
                self.stack_mut().pop();
            }
            Some(b'>') => self.push(&value[1..]),
            Some(b'=') => self.set(&value[1..]),
            _ => self.set(value),
        }
    }

    fn push(&mut self, names: &str) {
        for shape in names.split(',').filter_map(PointerShape::from_name) {
            let stack = self.stack_mut();
            if stack.len() == STACK_LIMIT {
                stack.remove(0);
            }
            stack.push(shape);
        }
    }

    fn set(&mut self, names: &str) {
        if names.is_empty() {
            self.stack_mut().clear();
            return;
        }
        // Setting replaces the top entry; pushing is the operation that grows the stack.
        let Some(shape) = names.split(',').find_map(PointerShape::from_name) else {
            return;
        };
        let stack = self.stack_mut();
        if let Some(top) = stack.last_mut() {
            *top = shape;
        } else {
            stack.push(shape);
        }
    }

    fn query(&mut self, names: &str) {
        let values: Vec<_> = names.split(',').map(|name| self.query_name(name)).collect();
        self.response = Some(format!("\x1b]22;{}\x1b\\", values.join(",")).into_bytes());
    }

    fn query_name(&self, name: &str) -> &'static str {
        match name {
            "__current__" => self.current().map_or("0", PointerShape::as_str),
            "__default__" => "text",
            "__grabbed__" => "default",
            _ => {
                if PointerShape::from_name(name).is_some() {
                    "1"
                } else {
                    "0"
                }
            }
        }
    }

    pub fn write_stack(&self, writer: &mut impl Write, alternate: bool) -> io::Result<()> {
        writer.write_all(b"\x1b]22;\x1b\\")?;
        for shape in self.stack(alternate) {
            write!(writer, "\x1b]22;>{}\x1b\\", shape.as_str())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::TerminalScreen;

    fn query(screen: &mut TerminalScreen, names: &str) -> Vec<Vec<u8>> {
        screen.process_bytes(format!("\x1b]22;?{names}\x1b\\").as_bytes());
        screen.drain_responses()
    }

    #[test]
    fn fragmented_pointer_requests_and_queries_preserve_response_order() {
        let mut screen = TerminalScreen::new(4, 20, 0);
        screen.render_snapshot();
        screen.take_mutation();
        screen.process_bytes(b"\x1b]2");
        screen.process_bytes(b"2;poi");
        let mutation = screen.process_bytes(b"nter\x07");
        assert!(mutation.modes_changed);
        assert_eq!(mutation.cells, crate::widgets::TerminalDamage::None);
        assert_eq!(screen.pointer_shape(), Some(PointerShape::Pointer));
        assert_eq!(
            screen.render_snapshot().pointer_shape,
            Some(PointerShape::Pointer)
        );
        screen.process_bytes(b"\x1b]22;?text,grab,bogus\x1b\\\x1b[5n\x1b]22;?__current__\x07");
        assert_eq!(
            screen.drain_responses(),
            vec![
                b"\x1b]22;1,1,0\x1b\\".to_vec(),
                b"\x1b[0n".to_vec(),
                b"\x1b]22;pointer\x1b\\".to_vec(),
            ]
        );
    }

    #[test]
    fn pointer_stacks_are_bounded_and_set_replaces_only_the_top() {
        let mut state = PointerState::default();
        state.dispatch(b">text,grab");
        state.dispatch(b"=pointer");
        state.dispatch(b"<");
        assert_eq!(state.current(), Some(PointerShape::Text));
        state.dispatch(b"bogus");
        assert_eq!(state.current(), Some(PointerShape::Text));
        for _ in 0..50 {
            state.dispatch(b">wait");
        }
        assert_eq!(state.primary.len(), STACK_LIMIT);
        for _ in 0..50 {
            state.dispatch(b"<");
        }
        assert_eq!(state.current(), None);
        state.dispatch(b">text,grab");
        state.dispatch(b"");
        assert_eq!(state.current(), None);
    }

    #[test]
    fn alternate_stacks_survive_replay_and_pop_after_reattaching() {
        let mut source = TerminalScreen::new(4, 20, 0);
        source.process_bytes(b"\x1b]22;>text,grab\x07\x1b[?1049h\x1b]22;>wait,pointer\x07");
        let bytes = source.export_replay_bytes();
        assert_eq!(source.pointer_shape(), Some(PointerShape::Pointer));
        let mut target = TerminalScreen::new(4, 20, 0);
        target.process_bytes(&bytes);
        assert_eq!(target.pointer_shape(), Some(PointerShape::Pointer));
        target.process_bytes(b"\x1b]22;<\x07");
        assert_eq!(target.pointer_shape(), Some(PointerShape::Wait));
        target.process_bytes(b"\x1b[?1049l");
        assert_eq!(target.pointer_shape(), Some(PointerShape::Grab));
        target.process_bytes(b"\x1b]22;<\x07");
        assert_eq!(target.pointer_shape(), Some(PointerShape::Text));
        assert!(target.drain_responses().is_empty());
    }

    #[test]
    fn hard_resets_clear_both_pointer_stacks() {
        let mut screen = TerminalScreen::new(4, 20, 0);
        screen.process_bytes(b"\x1b]22;grab\x07\x1b[?1049h\x1b]22;wait\x07\x1bc");
        assert_eq!(
            query(&mut screen, "__current__"),
            vec![b"\x1b]22;0\x1b\\".to_vec()]
        );
        screen.process_bytes(b"\x1b[?1049h");
        assert_eq!(screen.pointer_shape(), None);
        screen.process_bytes(b"\x1b]22;pointer\x07");
        screen.process_bytes(b"\x1b[!p");
        assert_eq!(screen.pointer_shape(), None);
        screen.process_bytes(b"\x1b]22;pointer\x07");
        screen.reset();
        assert_eq!(screen.pointer_shape(), None);
    }
}
