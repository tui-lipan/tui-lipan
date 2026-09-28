//! Handles naming render-resolved animations.

/// Names a render-resolved animation in the runtime's animation registry.
///
/// Carried by [`Paint::Animated`](crate::style::Paint::Animated) and late-bound
/// [`EffectAmount`](crate::style::EffectAmount)s so the renderer can look up the animation's
/// current value while painting. Opaque: handles come from
/// [`Context::animated_color`](crate::Context::animated_color),
/// [`Context::animated_amount`](crate::Context::animated_amount), and
/// [`Context::pulsing_amount`](crate::Context::pulsing_amount).
///
/// A handle is a registry slot plus the generation the slot had when the handle was minted. Slots
/// are recycled once their animation is dropped, and each reuse bumps the generation, so a handle
/// left behind in an old element tree stops resolving instead of naming whatever animation took
/// its slot next.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationHandle {
    slot: u16,
    generation: u16,
}

impl AnimationHandle {
    /// Width of the generation counter. Fourteen bits leave room for two tag bits beside a handle
    /// packed into one `u32`, which keeps an `EffectAmount` at eight bytes.
    pub(crate) const GENERATION_BITS: u32 = 14;
    const GENERATION_MASK: u16 = (1 << Self::GENERATION_BITS) - 1;

    pub(crate) fn new(slot: u16, generation: u16) -> Self {
        Self {
            slot,
            generation: generation & Self::GENERATION_MASK,
        }
    }

    pub(crate) fn slot(self) -> u16 {
        self.slot
    }

    pub(crate) fn generation(self) -> u16 {
        self.generation
    }

    /// The generation a slot gets on its next reuse.
    pub(crate) fn next_generation(generation: u16) -> u16 {
        generation.wrapping_add(1) & Self::GENERATION_MASK
    }

    /// This handle in the low 30 bits of a `u32`.
    pub(crate) fn pack(self) -> u32 {
        u32::from(self.slot) | (u32::from(self.generation) << 16)
    }

    /// The handle in the low 30 bits of `bits`; the top two bits are ignored.
    pub(crate) fn unpack(bits: u32) -> Self {
        Self::new(bits as u16, (bits >> 16) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_round_trips_through_its_packed_form() {
        let handle = AnimationHandle::new(0xBEEF, 0x3ABC);
        assert_eq!(AnimationHandle::unpack(handle.pack()), handle);
        assert_eq!(handle.pack() >> 30, 0, "the top two bits stay free");
    }

    #[test]
    fn generations_wrap_within_their_width() {
        let last = (1 << AnimationHandle::GENERATION_BITS) - 1;
        assert_eq!(AnimationHandle::next_generation(last), 0);
    }
}
