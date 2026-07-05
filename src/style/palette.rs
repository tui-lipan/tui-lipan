//! A comprehensive color palette based on Tailwind CSS.
//!
//! Top-level constants provide quick access to 500-series colors for common use.
//! Each color family also has a nested module with all shades (50-950) for fine-grained control.

use super::Color;

// Top-level convenience constants (500-series)

/// Slate 500 - #64748b
pub const SLATE: Color = Color::rgb(100, 116, 139);
/// Gray 500 - #6b7280
pub const GRAY: Color = Color::rgb(107, 114, 128);
/// Zinc 500 - #71717a
pub const ZINC: Color = Color::rgb(113, 113, 122);
/// Neutral 500 - #737373
pub const NEUTRAL: Color = Color::rgb(115, 115, 115);
/// Stone 500 - #78716c
pub const STONE: Color = Color::rgb(120, 113, 108);
/// Red 500 - #ef4444
pub const RED: Color = Color::rgb(239, 68, 68);
/// Orange 500 - #f97316
pub const ORANGE: Color = Color::rgb(249, 115, 22);
/// Amber 500 - #f59e0b
pub const AMBER: Color = Color::rgb(245, 158, 11);
/// Yellow 500 - #eab308
pub const YELLOW: Color = Color::rgb(234, 179, 8);
/// Lime 500 - #84cc16
pub const LIME: Color = Color::rgb(132, 204, 22);
/// Green 500 - #22c55e
pub const GREEN: Color = Color::rgb(34, 197, 94);
/// Emerald 500 - #10b981
pub const EMERALD: Color = Color::rgb(16, 185, 129);
/// Teal 500 - #14b8a6
pub const TEAL: Color = Color::rgb(20, 184, 166);
/// Cyan 500 - #06b6d4
pub const CYAN: Color = Color::rgb(6, 182, 212);
/// Sky 500 - #0ea5e9
pub const SKY: Color = Color::rgb(14, 165, 233);
/// Blue 500 - #3b82f6
pub const BLUE: Color = Color::rgb(59, 130, 246);
/// Indigo 500 - #6366f1
pub const INDIGO: Color = Color::rgb(99, 102, 241);
/// Violet 500 - #8b5cf6
pub const VIOLET: Color = Color::rgb(139, 92, 246);
/// Purple 500 - #a855f7
pub const PURPLE: Color = Color::rgb(168, 85, 247);
/// Fuchsia 500 - #d946ef
pub const FUCHSIA: Color = Color::rgb(217, 70, 239);
/// Pink 500 - #ec4899
pub const PINK: Color = Color::rgb(236, 72, 153);
/// Rose 500 - #f43f5e
pub const ROSE: Color = Color::rgb(244, 63, 94);

/// Slate color family (cool gray).
pub mod slate {
    use super::Color;
    /// Slate 50 - #f8fafc
    pub const B50: Color = Color::rgb(248, 250, 252);
    /// Slate 100 - #f1f5f9
    pub const B100: Color = Color::rgb(241, 245, 249);
    /// Slate 200 - #e2e8f0
    pub const B200: Color = Color::rgb(226, 232, 240);
    /// Slate 300 - #cbd5e1
    pub const B300: Color = Color::rgb(203, 213, 225);
    /// Slate 400 - #94a3b8
    pub const B400: Color = Color::rgb(148, 163, 184);
    /// Slate 500 - #64748b
    pub const B500: Color = Color::rgb(100, 116, 139);
    /// Slate 600 - #475569
    pub const B600: Color = Color::rgb(71, 85, 105);
    /// Slate 700 - #334155
    pub const B700: Color = Color::rgb(51, 65, 85);
    /// Slate 800 - #1e293b
    pub const B800: Color = Color::rgb(30, 41, 59);
    /// Slate 900 - #0f172a
    pub const B900: Color = Color::rgb(15, 23, 42);
    /// Slate 950 - #020617
    pub const B950: Color = Color::rgb(2, 6, 23);
}

/// Gray color family (neutral gray).
pub mod gray {
    use super::Color;
    /// Gray 50 - #f9fafb
    pub const B50: Color = Color::rgb(249, 250, 251);
    /// Gray 100 - #f3f4f6
    pub const B100: Color = Color::rgb(243, 244, 246);
    /// Gray 200 - #e5e7eb
    pub const B200: Color = Color::rgb(229, 231, 235);
    /// Gray 300 - #d1d5db
    pub const B300: Color = Color::rgb(209, 213, 219);
    /// Gray 400 - #9ca3af
    pub const B400: Color = Color::rgb(156, 163, 175);
    /// Gray 500 - #6b7280
    pub const B500: Color = Color::rgb(107, 114, 128);
    /// Gray 600 - #4b5563
    pub const B600: Color = Color::rgb(75, 85, 99);
    /// Gray 700 - #374151
    pub const B700: Color = Color::rgb(55, 65, 81);
    /// Gray 800 - #1f2937
    pub const B800: Color = Color::rgb(31, 41, 55);
    /// Gray 900 - #111827
    pub const B900: Color = Color::rgb(17, 24, 39);
    /// Gray 950 - #030712
    pub const B950: Color = Color::rgb(3, 7, 18);
}

/// Zinc color family (cool neutral).
pub mod zinc {
    use super::Color;
    /// Zinc 50 - #fafafa
    pub const B50: Color = Color::rgb(250, 250, 250);
    /// Zinc 100 - #f4f4f5
    pub const B100: Color = Color::rgb(244, 244, 245);
    /// Zinc 200 - #e4e4e7
    pub const B200: Color = Color::rgb(228, 228, 231);
    /// Zinc 300 - #d4d4d8
    pub const B300: Color = Color::rgb(212, 212, 216);
    /// Zinc 400 - #a1a1aa
    pub const B400: Color = Color::rgb(161, 161, 170);
    /// Zinc 500 - #71717a
    pub const B500: Color = Color::rgb(113, 113, 122);
    /// Zinc 600 - #52525b
    pub const B600: Color = Color::rgb(82, 82, 91);
    /// Zinc 700 - #3f3f46
    pub const B700: Color = Color::rgb(63, 63, 70);
    /// Zinc 800 - #27272a
    pub const B800: Color = Color::rgb(39, 39, 42);
    /// Zinc 900 - #18181b
    pub const B900: Color = Color::rgb(24, 24, 27);
    /// Zinc 950 - #09090b
    pub const B950: Color = Color::rgb(9, 9, 11);
}

/// Neutral color family (true gray).
pub mod neutral {
    use super::Color;
    /// Neutral 50 - #fafafa
    pub const B50: Color = Color::rgb(250, 250, 250);
    /// Neutral 100 - #f5f5f5
    pub const B100: Color = Color::rgb(245, 245, 245);
    /// Neutral 200 - #e5e5e5
    pub const B200: Color = Color::rgb(229, 229, 229);
    /// Neutral 300 - #d4d4d4
    pub const B300: Color = Color::rgb(212, 212, 212);
    /// Neutral 400 - #a3a3a3
    pub const B400: Color = Color::rgb(163, 163, 163);
    /// Neutral 500 - #737373
    pub const B500: Color = Color::rgb(115, 115, 115);
    /// Neutral 600 - #525252
    pub const B600: Color = Color::rgb(82, 82, 82);
    /// Neutral 700 - #404040
    pub const B700: Color = Color::rgb(64, 64, 64);
    /// Neutral 800 - #262626
    pub const B800: Color = Color::rgb(38, 38, 38);
    /// Neutral 900 - #171717
    pub const B900: Color = Color::rgb(23, 23, 23);
    /// Neutral 950 - #0a0a0a
    pub const B950: Color = Color::rgb(10, 10, 10);
}

/// Stone color family (warm gray).
pub mod stone {
    use super::Color;
    /// Stone 50 - #fafaf9
    pub const B50: Color = Color::rgb(250, 250, 249);
    /// Stone 100 - #f5f5f4
    pub const B100: Color = Color::rgb(245, 245, 244);
    /// Stone 200 - #e7e5e4
    pub const B200: Color = Color::rgb(231, 229, 228);
    /// Stone 300 - #d6d3d1
    pub const B300: Color = Color::rgb(214, 211, 209);
    /// Stone 400 - #a8a29e
    pub const B400: Color = Color::rgb(168, 162, 158);
    /// Stone 500 - #78716c
    pub const B500: Color = Color::rgb(120, 113, 108);
    /// Stone 600 - #57534e
    pub const B600: Color = Color::rgb(87, 83, 78);
    /// Stone 700 - #44403c
    pub const B700: Color = Color::rgb(68, 64, 60);
    /// Stone 800 - #292524
    pub const B800: Color = Color::rgb(41, 37, 36);
    /// Stone 900 - #1c1917
    pub const B900: Color = Color::rgb(28, 25, 23);
    /// Stone 950 - #0c0a09
    pub const B950: Color = Color::rgb(12, 10, 9);
}

/// Red color family.
pub mod red {
    use super::Color;
    /// Red 50 - #fef2f2
    pub const B50: Color = Color::rgb(254, 242, 242);
    /// Red 100 - #fee2e2
    pub const B100: Color = Color::rgb(254, 226, 226);
    /// Red 200 - #fecaca
    pub const B200: Color = Color::rgb(254, 202, 202);
    /// Red 300 - #fca5a5
    pub const B300: Color = Color::rgb(252, 165, 165);
    /// Red 400 - #f87171
    pub const B400: Color = Color::rgb(248, 113, 113);
    /// Red 500 - #ef4444
    pub const B500: Color = Color::rgb(239, 68, 68);
    /// Red 600 - #dc2626
    pub const B600: Color = Color::rgb(220, 38, 38);
    /// Red 700 - #b91c1c
    pub const B700: Color = Color::rgb(185, 28, 28);
    /// Red 800 - #991b1b
    pub const B800: Color = Color::rgb(153, 27, 27);
    /// Red 900 - #7f1d1d
    pub const B900: Color = Color::rgb(127, 29, 29);
    /// Red 950 - #450a0a
    pub const B950: Color = Color::rgb(69, 10, 10);
}

/// Orange color family.
pub mod orange {
    use super::Color;
    /// Orange 50 - #fff7ed
    pub const B50: Color = Color::rgb(255, 247, 237);
    /// Orange 100 - #ffedd5
    pub const B100: Color = Color::rgb(255, 237, 213);
    /// Orange 200 - #fed7aa
    pub const B200: Color = Color::rgb(254, 215, 170);
    /// Orange 300 - #fdba74
    pub const B300: Color = Color::rgb(253, 186, 116);
    /// Orange 400 - #fb923c
    pub const B400: Color = Color::rgb(251, 146, 60);
    /// Orange 500 - #f97316
    pub const B500: Color = Color::rgb(249, 115, 22);
    /// Orange 600 - #ea580c
    pub const B600: Color = Color::rgb(234, 88, 12);
    /// Orange 700 - #c2410c
    pub const B700: Color = Color::rgb(194, 65, 12);
    /// Orange 800 - #9a3412
    pub const B800: Color = Color::rgb(154, 52, 18);
    /// Orange 900 - #7c2d12
    pub const B900: Color = Color::rgb(124, 45, 18);
    /// Orange 950 - #431407
    pub const B950: Color = Color::rgb(67, 20, 7);
}

/// Amber color family.
pub mod amber {
    use super::Color;
    /// Amber 50 - #fffbeb
    pub const B50: Color = Color::rgb(255, 251, 235);
    /// Amber 100 - #fef3c7
    pub const B100: Color = Color::rgb(254, 243, 199);
    /// Amber 200 - #fde68a
    pub const B200: Color = Color::rgb(253, 230, 138);
    /// Amber 300 - #fcd34d
    pub const B300: Color = Color::rgb(252, 211, 77);
    /// Amber 400 - #fbbf24
    pub const B400: Color = Color::rgb(251, 191, 36);
    /// Amber 500 - #f59e0b
    pub const B500: Color = Color::rgb(245, 158, 11);
    /// Amber 600 - #d97706
    pub const B600: Color = Color::rgb(217, 119, 6);
    /// Amber 700 - #b45309
    pub const B700: Color = Color::rgb(180, 83, 9);
    /// Amber 800 - #92400e
    pub const B800: Color = Color::rgb(146, 64, 14);
    /// Amber 900 - #78350f
    pub const B900: Color = Color::rgb(120, 53, 15);
    /// Amber 950 - #451a03
    pub const B950: Color = Color::rgb(69, 26, 3);
}

/// Yellow color family.
pub mod yellow {
    use super::Color;
    /// Yellow 50 - #fefce8
    pub const B50: Color = Color::rgb(254, 252, 232);
    /// Yellow 100 - #fef9c3
    pub const B100: Color = Color::rgb(254, 249, 195);
    /// Yellow 200 - #fef08a
    pub const B200: Color = Color::rgb(254, 240, 138);
    /// Yellow 300 - #fde047
    pub const B300: Color = Color::rgb(253, 224, 71);
    /// Yellow 400 - #facc15
    pub const B400: Color = Color::rgb(250, 204, 21);
    /// Yellow 500 - #eab308
    pub const B500: Color = Color::rgb(234, 179, 8);
    /// Yellow 600 - #ca8a04
    pub const B600: Color = Color::rgb(202, 138, 4);
    /// Yellow 700 - #a16207
    pub const B700: Color = Color::rgb(161, 98, 7);
    /// Yellow 800 - #854d0e
    pub const B800: Color = Color::rgb(133, 77, 14);
    /// Yellow 900 - #713f12
    pub const B900: Color = Color::rgb(113, 63, 18);
    /// Yellow 950 - #422006
    pub const B950: Color = Color::rgb(66, 32, 6);
}

/// Lime color family.
pub mod lime {
    use super::Color;
    /// Lime 50 - #f7fee7
    pub const B50: Color = Color::rgb(247, 254, 231);
    /// Lime 100 - #ecfccb
    pub const B100: Color = Color::rgb(236, 252, 203);
    /// Lime 200 - #d9f99d
    pub const B200: Color = Color::rgb(217, 249, 157);
    /// Lime 300 - #bef264
    pub const B300: Color = Color::rgb(190, 242, 100);
    /// Lime 400 - #a3e635
    pub const B400: Color = Color::rgb(163, 230, 53);
    /// Lime 500 - #84cc16
    pub const B500: Color = Color::rgb(132, 204, 22);
    /// Lime 600 - #65a30d
    pub const B600: Color = Color::rgb(101, 163, 13);
    /// Lime 700 - #4d7c0f
    pub const B700: Color = Color::rgb(77, 124, 15);
    /// Lime 800 - #3f6212
    pub const B800: Color = Color::rgb(63, 98, 18);
    /// Lime 900 - #365314
    pub const B900: Color = Color::rgb(54, 83, 20);
    /// Lime 950 - #1a2e05
    pub const B950: Color = Color::rgb(26, 46, 5);
}

/// Green color family.
pub mod green {
    use super::Color;
    /// Green 50 - #f0fdf4
    pub const B50: Color = Color::rgb(240, 253, 244);
    /// Green 100 - #dcfce7
    pub const B100: Color = Color::rgb(220, 252, 231);
    /// Green 200 - #bbf7d0
    pub const B200: Color = Color::rgb(187, 247, 208);
    /// Green 300 - #86efac
    pub const B300: Color = Color::rgb(134, 239, 172);
    /// Green 400 - #4ade80
    pub const B400: Color = Color::rgb(74, 222, 128);
    /// Green 500 - #22c55e
    pub const B500: Color = Color::rgb(34, 197, 94);
    /// Green 600 - #16a34a
    pub const B600: Color = Color::rgb(22, 163, 74);
    /// Green 700 - #15803d
    pub const B700: Color = Color::rgb(21, 128, 61);
    /// Green 800 - #166534
    pub const B800: Color = Color::rgb(22, 101, 52);
    /// Green 900 - #14532d
    pub const B900: Color = Color::rgb(20, 83, 45);
    /// Green 950 - #052e16
    pub const B950: Color = Color::rgb(5, 46, 22);
}

/// Emerald color family.
pub mod emerald {
    use super::Color;
    /// Emerald 50 - #ecfdf5
    pub const B50: Color = Color::rgb(236, 253, 245);
    /// Emerald 100 - #d1fae5
    pub const B100: Color = Color::rgb(209, 250, 229);
    /// Emerald 200 - #a7f3d0
    pub const B200: Color = Color::rgb(167, 243, 208);
    /// Emerald 300 - #6ee7b7
    pub const B300: Color = Color::rgb(110, 231, 183);
    /// Emerald 400 - #34d399
    pub const B400: Color = Color::rgb(52, 211, 153);
    /// Emerald 500 - #10b981
    pub const B500: Color = Color::rgb(16, 185, 129);
    /// Emerald 600 - #059669
    pub const B600: Color = Color::rgb(5, 150, 105);
    /// Emerald 700 - #047857
    pub const B700: Color = Color::rgb(4, 120, 87);
    /// Emerald 800 - #065f46
    pub const B800: Color = Color::rgb(6, 95, 70);
    /// Emerald 900 - #064e3b
    pub const B900: Color = Color::rgb(6, 78, 59);
    /// Emerald 950 - #022c22
    pub const B950: Color = Color::rgb(2, 44, 34);
}

/// Teal color family.
pub mod teal {
    use super::Color;
    /// Teal 50 - #f0fdfa
    pub const B50: Color = Color::rgb(240, 253, 250);
    /// Teal 100 - #ccfbf1
    pub const B100: Color = Color::rgb(204, 251, 241);
    /// Teal 200 - #99f6e4
    pub const B200: Color = Color::rgb(153, 246, 228);
    /// Teal 300 - #5eead4
    pub const B300: Color = Color::rgb(94, 234, 212);
    /// Teal 400 - #2dd4bf
    pub const B400: Color = Color::rgb(45, 212, 191);
    /// Teal 500 - #14b8a6
    pub const B500: Color = Color::rgb(20, 184, 166);
    /// Teal 600 - #0d9488
    pub const B600: Color = Color::rgb(13, 148, 136);
    /// Teal 700 - #0f766e
    pub const B700: Color = Color::rgb(15, 118, 110);
    /// Teal 800 - #115e59
    pub const B800: Color = Color::rgb(17, 94, 89);
    /// Teal 900 - #134e4a
    pub const B900: Color = Color::rgb(19, 78, 74);
    /// Teal 950 - #042f2e
    pub const B950: Color = Color::rgb(4, 47, 46);
}

/// Cyan color family.
pub mod cyan {
    use super::Color;
    /// Cyan 50 - #ecfeff
    pub const B50: Color = Color::rgb(236, 254, 255);
    /// Cyan 100 - #cffafe
    pub const B100: Color = Color::rgb(207, 250, 254);
    /// Cyan 200 - #a5f3fc
    pub const B200: Color = Color::rgb(165, 243, 252);
    /// Cyan 300 - #67e8f9
    pub const B300: Color = Color::rgb(103, 232, 249);
    /// Cyan 400 - #22d3ee
    pub const B400: Color = Color::rgb(34, 211, 238);
    /// Cyan 500 - #06b6d4
    pub const B500: Color = Color::rgb(6, 182, 212);
    /// Cyan 600 - #0891b2
    pub const B600: Color = Color::rgb(8, 145, 178);
    /// Cyan 700 - #0e7490
    pub const B700: Color = Color::rgb(14, 116, 144);
    /// Cyan 800 - #155e75
    pub const B800: Color = Color::rgb(21, 94, 117);
    /// Cyan 900 - #164e63
    pub const B900: Color = Color::rgb(22, 78, 99);
    /// Cyan 950 - #083344
    pub const B950: Color = Color::rgb(8, 51, 68);
}

/// Sky color family.
pub mod sky {
    use super::Color;
    /// Sky 50 - #f0f9ff
    pub const B50: Color = Color::rgb(240, 249, 255);
    /// Sky 100 - #e0f2fe
    pub const B100: Color = Color::rgb(224, 242, 254);
    /// Sky 200 - #bae6fd
    pub const B200: Color = Color::rgb(186, 230, 253);
    /// Sky 300 - #7dd3fc
    pub const B300: Color = Color::rgb(125, 211, 252);
    /// Sky 400 - #38bdf8
    pub const B400: Color = Color::rgb(56, 189, 248);
    /// Sky 500 - #0ea5e9
    pub const B500: Color = Color::rgb(14, 165, 233);
    /// Sky 600 - #0284c7
    pub const B600: Color = Color::rgb(2, 132, 199);
    /// Sky 700 - #0369a1
    pub const B700: Color = Color::rgb(3, 105, 161);
    /// Sky 800 - #075985
    pub const B800: Color = Color::rgb(7, 89, 133);
    /// Sky 900 - #0c4a6e
    pub const B900: Color = Color::rgb(12, 74, 110);
    /// Sky 950 - #082f49
    pub const B950: Color = Color::rgb(8, 47, 73);
}

/// Blue color family.
pub mod blue {
    use super::Color;
    /// Blue 50 - #eff6ff
    pub const B50: Color = Color::rgb(239, 246, 255);
    /// Blue 100 - #dbeafe
    pub const B100: Color = Color::rgb(219, 234, 254);
    /// Blue 200 - #bfdbfe
    pub const B200: Color = Color::rgb(191, 219, 254);
    /// Blue 300 - #93c5fd
    pub const B300: Color = Color::rgb(147, 197, 253);
    /// Blue 400 - #60a5fa
    pub const B400: Color = Color::rgb(96, 165, 250);
    /// Blue 500 - #3b82f6
    pub const B500: Color = Color::rgb(59, 130, 246);
    /// Blue 600 - #2563eb
    pub const B600: Color = Color::rgb(37, 99, 235);
    /// Blue 700 - #1d4ed8
    pub const B700: Color = Color::rgb(29, 78, 216);
    /// Blue 800 - #1e40af
    pub const B800: Color = Color::rgb(30, 64, 175);
    /// Blue 900 - #1e3a8a
    pub const B900: Color = Color::rgb(30, 58, 138);
    /// Blue 950 - #172554
    pub const B950: Color = Color::rgb(23, 37, 84);
}

/// Indigo color family.
pub mod indigo {
    use super::Color;
    /// Indigo 50 - #eef2ff
    pub const B50: Color = Color::rgb(238, 242, 255);
    /// Indigo 100 - #e0e7ff
    pub const B100: Color = Color::rgb(224, 231, 255);
    /// Indigo 200 - #c7d2fe
    pub const B200: Color = Color::rgb(199, 210, 254);
    /// Indigo 300 - #a5b4fc
    pub const B300: Color = Color::rgb(165, 180, 252);
    /// Indigo 400 - #818cf8
    pub const B400: Color = Color::rgb(129, 140, 248);
    /// Indigo 500 - #6366f1
    pub const B500: Color = Color::rgb(99, 102, 241);
    /// Indigo 600 - #4f46e5
    pub const B600: Color = Color::rgb(79, 70, 229);
    /// Indigo 700 - #4338ca
    pub const B700: Color = Color::rgb(67, 56, 202);
    /// Indigo 800 - #3730a3
    pub const B800: Color = Color::rgb(55, 48, 163);
    /// Indigo 900 - #312e81
    pub const B900: Color = Color::rgb(49, 46, 129);
    /// Indigo 950 - #1e1b4b
    pub const B950: Color = Color::rgb(30, 27, 75);
}

/// Violet color family.
pub mod violet {
    use super::Color;
    /// Violet 50 - #f5f3ff
    pub const B50: Color = Color::rgb(245, 243, 255);
    /// Violet 100 - #ede9fe
    pub const B100: Color = Color::rgb(237, 233, 254);
    /// Violet 200 - #ddd6fe
    pub const B200: Color = Color::rgb(221, 214, 254);
    /// Violet 300 - #c4b5fd
    pub const B300: Color = Color::rgb(196, 181, 253);
    /// Violet 400 - #a78bfa
    pub const B400: Color = Color::rgb(167, 139, 250);
    /// Violet 500 - #8b5cf6
    pub const B500: Color = Color::rgb(139, 92, 246);
    /// Violet 600 - #7c3aed
    pub const B600: Color = Color::rgb(124, 58, 237);
    /// Violet 700 - #6d28d9
    pub const B700: Color = Color::rgb(109, 40, 217);
    /// Violet 800 - #5b21b6
    pub const B800: Color = Color::rgb(91, 33, 182);
    /// Violet 900 - #4c1d95
    pub const B900: Color = Color::rgb(76, 29, 149);
    /// Violet 950 - #2e1065
    pub const B950: Color = Color::rgb(46, 16, 101);
}

/// Purple color family.
pub mod purple {
    use super::Color;
    /// Purple 50 - #faf5ff
    pub const B50: Color = Color::rgb(250, 245, 255);
    /// Purple 100 - #f3e8ff
    pub const B100: Color = Color::rgb(243, 232, 255);
    /// Purple 200 - #e9d5ff
    pub const B200: Color = Color::rgb(233, 213, 255);
    /// Purple 300 - #d8b4fe
    pub const B300: Color = Color::rgb(216, 180, 254);
    /// Purple 400 - #c084fc
    pub const B400: Color = Color::rgb(192, 132, 252);
    /// Purple 500 - #a855f7
    pub const B500: Color = Color::rgb(168, 85, 247);
    /// Purple 600 - #9333ea
    pub const B600: Color = Color::rgb(147, 51, 234);
    /// Purple 700 - #7e22ce
    pub const B700: Color = Color::rgb(126, 34, 206);
    /// Purple 800 - #6b21a8
    pub const B800: Color = Color::rgb(107, 33, 168);
    /// Purple 900 - #581c87
    pub const B900: Color = Color::rgb(88, 28, 135);
    /// Purple 950 - #3b0764
    pub const B950: Color = Color::rgb(59, 7, 100);
}

/// Fuchsia color family.
pub mod fuchsia {
    use super::Color;
    /// Fuchsia 50 - #fdf4ff
    pub const B50: Color = Color::rgb(253, 244, 255);
    /// Fuchsia 100 - #fae8ff
    pub const B100: Color = Color::rgb(250, 232, 255);
    /// Fuchsia 200 - #f5d0fe
    pub const B200: Color = Color::rgb(245, 208, 254);
    /// Fuchsia 300 - #f0abfc
    pub const B300: Color = Color::rgb(240, 171, 252);
    /// Fuchsia 400 - #e879f9
    pub const B400: Color = Color::rgb(232, 121, 249);
    /// Fuchsia 500 - #d946ef
    pub const B500: Color = Color::rgb(217, 70, 239);
    /// Fuchsia 600 - #c026d3
    pub const B600: Color = Color::rgb(192, 38, 211);
    /// Fuchsia 700 - #a21caf
    pub const B700: Color = Color::rgb(162, 28, 175);
    /// Fuchsia 800 - #86198f
    pub const B800: Color = Color::rgb(134, 25, 143);
    /// Fuchsia 900 - #701a75
    pub const B900: Color = Color::rgb(112, 26, 117);
    /// Fuchsia 950 - #4a044e
    pub const B950: Color = Color::rgb(74, 4, 78);
}

/// Pink color family.
pub mod pink {
    use super::Color;
    /// Pink 50 - #fdf2f8
    pub const B50: Color = Color::rgb(253, 242, 248);
    /// Pink 100 - #fce7f3
    pub const B100: Color = Color::rgb(252, 231, 243);
    /// Pink 200 - #fbcfe8
    pub const B200: Color = Color::rgb(251, 207, 232);
    /// Pink 300 - #f9a8d4
    pub const B300: Color = Color::rgb(249, 168, 212);
    /// Pink 400 - #f472b6
    pub const B400: Color = Color::rgb(244, 114, 182);
    /// Pink 500 - #ec4899
    pub const B500: Color = Color::rgb(236, 72, 153);
    /// Pink 600 - #db2777
    pub const B600: Color = Color::rgb(219, 39, 119);
    /// Pink 700 - #be185d
    pub const B700: Color = Color::rgb(190, 24, 93);
    /// Pink 800 - #9d174d
    pub const B800: Color = Color::rgb(157, 23, 77);
    /// Pink 900 - #831843
    pub const B900: Color = Color::rgb(131, 24, 67);
    /// Pink 950 - #500724
    pub const B950: Color = Color::rgb(80, 7, 36);
}

/// Rose color family.
pub mod rose {
    use super::Color;
    /// Rose 50 - #fff1f2
    pub const B50: Color = Color::rgb(255, 241, 242);
    /// Rose 100 - #ffe4e6
    pub const B100: Color = Color::rgb(255, 228, 230);
    /// Rose 200 - #fecdd3
    pub const B200: Color = Color::rgb(254, 205, 211);
    /// Rose 300 - #fda4af
    pub const B300: Color = Color::rgb(253, 164, 175);
    /// Rose 400 - #fb7185
    pub const B400: Color = Color::rgb(251, 113, 133);
    /// Rose 500 - #f43f5e
    pub const B500: Color = Color::rgb(244, 63, 94);
    /// Rose 600 - #e11d48
    pub const B600: Color = Color::rgb(225, 29, 72);
    /// Rose 700 - #be123c
    pub const B700: Color = Color::rgb(190, 18, 60);
    /// Rose 800 - #9f1239
    pub const B800: Color = Color::rgb(159, 18, 57);
    /// Rose 900 - #881337
    pub const B900: Color = Color::rgb(136, 19, 55);
    /// Rose 950 - #4c0519
    pub const B950: Color = Color::rgb(76, 5, 25);
}
