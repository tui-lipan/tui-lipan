use crate::widgets::{BigFont, BigText};

// Captured from figlet-rs 1.0.0 before removing the dependency. FNV-1a hashes cover the
// exact rendered UTF-8 bytes, including whitespace and the previous fallback behavior.
#[test]
fn bundled_fonts_preserve_upstream_rendering() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("output-golden.json")).unwrap();
    for case in cases {
        let font = match case["font"].as_str().unwrap() {
            "AnsiShadow.flf" => BigFont::AnsiShadow,
            "Bloody.flf" => BigFont::Bloody,
            "Colossal.flf" => BigFont::Colossal,
            "DosRebel.flf" => BigFont::DosRebel,
            "Nancyj.flf" => BigFont::Nancyj,
            "Poison.flf" => BigFont::Poison,
            "Roman.flf" => BigFont::Roman,
            "Slant.flf" => BigFont::Slant,
            "Small.flf" => BigFont::Small,
            "SmallPoison.flf" => BigFont::SmallPoison,
            "Standard.flf" => BigFont::Standard,
            "SubZero.flf" => BigFont::SubZero,
            other => panic!("unexpected golden font: {other}"),
        };
        let text = case["text"].as_str().unwrap();
        let rendered = BigText::new().font(font).render_figlet(text).join("\n");
        let hash = rendered.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(hash, case["hash"].as_u64().unwrap(), "{font:?}: {text:?}");
    }
}

#[test]
fn custom_fonts_share_the_engine_and_parse_failures_remain_nonfatal() {
    let text = "Rozi";
    let custom = BigText::new()
        .font(BigFont::CustomFiglet)
        .custom_figlet(super::super::font::SMALL_FONT)
        .render_figlet(text);
    assert_eq!(
        custom,
        BigText::new().font(BigFont::Small).render_figlet(text)
    );
    let invalid = BigText::new()
        .font(BigFont::CustomFiglet)
        .custom_figlet("not a font")
        .render_figlet(text);
    assert_eq!(
        invalid,
        BigText::new().font(BigFont::Standard).render_figlet(text)
    );
}
