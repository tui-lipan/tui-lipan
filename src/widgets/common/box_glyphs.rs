use crate::style::BorderStyle;

pub(crate) const NORTH: u8 = 0b0001;
pub(crate) const SOUTH: u8 = 0b0010;
pub(crate) const WEST: u8 = 0b0100;
pub(crate) const EAST: u8 = 0b1000;
pub(crate) const ALL_DIRECTIONS: u8 = NORTH | SOUTH | WEST | EAST;

pub(crate) fn glyph_for_bits(bits: u8, border_style: BorderStyle) -> char {
    debug_assert_ne!(bits & ALL_DIRECTIONS, 0);
    let rounded = matches!(border_style, BorderStyle::Rounded);
    match bits & ALL_DIRECTIONS {
        b if b == (NORTH | SOUTH) => '│',
        b if b == (WEST | EAST) => '─',
        b if b == (SOUTH | EAST) && rounded => '╭',
        b if b == (SOUTH | EAST) => '┌',
        b if b == (SOUTH | WEST) && rounded => '╮',
        b if b == (SOUTH | WEST) => '┐',
        b if b == (NORTH | EAST) && rounded => '╰',
        b if b == (NORTH | EAST) => '└',
        b if b == (NORTH | WEST) && rounded => '╯',
        b if b == (NORTH | WEST) => '┘',
        b if b == (NORTH | SOUTH | EAST) => '├',
        b if b == (NORTH | SOUTH | WEST) => '┤',
        b if b == (SOUTH | WEST | EAST) => '┬',
        b if b == (NORTH | WEST | EAST) => '┴',
        b if b == (NORTH | SOUTH | WEST | EAST) => '┼',
        b if b == NORTH || b == SOUTH => '│',
        b if b == WEST || b == EAST => '─',
        _ => '─',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_plain_glyphs() {
        assert_eq!(glyph_for_bits(NORTH | SOUTH, BorderStyle::Plain), '│');
        assert_eq!(glyph_for_bits(WEST | EAST, BorderStyle::Plain), '─');
        assert_eq!(glyph_for_bits(SOUTH | EAST, BorderStyle::Plain), '┌');
        assert_eq!(glyph_for_bits(SOUTH | WEST, BorderStyle::Plain), '┐');
        assert_eq!(glyph_for_bits(NORTH | EAST, BorderStyle::Plain), '└');
        assert_eq!(glyph_for_bits(NORTH | WEST, BorderStyle::Plain), '┘');
        assert_eq!(
            glyph_for_bits(NORTH | SOUTH | WEST | EAST, BorderStyle::Plain),
            '┼'
        );
    }

    #[test]
    fn rounded_only_changes_elbows() {
        assert_eq!(glyph_for_bits(SOUTH | EAST, BorderStyle::Rounded), '╭');
        assert_eq!(glyph_for_bits(SOUTH | WEST, BorderStyle::Rounded), '╮');
        assert_eq!(glyph_for_bits(NORTH | EAST, BorderStyle::Rounded), '╰');
        assert_eq!(glyph_for_bits(NORTH | WEST, BorderStyle::Rounded), '╯');
        assert_eq!(
            glyph_for_bits(NORTH | SOUTH | EAST, BorderStyle::Rounded),
            '├'
        );
    }

    #[test]
    fn ignores_high_bits() {
        assert_eq!(
            glyph_for_bits(NORTH | SOUTH | 0b1000_0000, BorderStyle::Plain),
            '│'
        );
    }
}
