//! Sketch: fenced code-block presentation in `DocumentView`.

use tui_lipan::prelude::*;
#[cfg(feature = "markdown")]
use tui_lipan::{CodeBlockHeader, CodeBlockPadding};
use tui_lipan::{Result, Sketch};

#[cfg(feature = "markdown")]
const SAMPLE_MD: &str = r#"# Code Block Presentation

Inline `code` stays compact on prose lines while fenced blocks stand out:

```rust
fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}
```

And inside a list:

- Run the checks:
  ```sh
  cargo test --workspace
  ```

```diff
 fn greet(name: &str) -> String {
-    format!("Hi, {name}")
+    format!("Hello, {name}!")
 }
```
"#;

fn view() -> Element {
    let mut theme = Theme::one_dark();
    theme.document.code_block = Style::new().bg(Color::hex_u24(0x1E222A));

    #[cfg(feature = "markdown")]
    let body: Element = HStack::new()
        .gap(1)
        .padding(1)
        .child(
            Frame::new()
                .title("Code Block (Flex + HalfBlock + IconAndLabel)")
                .border(true)
                .width(Length::Flex(1))
                .height(Length::Flex(1))
                .child(
                    DocumentView::new(SAMPLE_MD)
                        .markdown()
                        .code_block_width(Length::Flex(1))
                        .code_block_padding(CodeBlockPadding::HalfBlock)
                        .code_block_header(CodeBlockHeader::IconAndLabel)
                        .scrollbar(false)
                        .wrap(true)
                        .border(false)
                        .padding(1),
                ),
        )
        .child(
            Frame::new()
                .title("Code Block (Auto + HalfBlock + IconAndLabel)")
                .border(true)
                .width(Length::Flex(1))
                .height(Length::Flex(1))
                .child(
                    DocumentView::new(SAMPLE_MD)
                        .markdown()
                        .code_block_width(Length::Auto)
                        .code_block_padding(CodeBlockPadding::HalfBlock)
                        .code_block_header(CodeBlockHeader::IconAndLabel)
                        .scrollbar(false)
                        .wrap(true)
                        .border(false)
                        .padding(1),
                ),
        )
        .into();

    #[cfg(not(feature = "markdown"))]
    let body: Element = Text::new("Enable `--features markdown` to preview code blocks.").into();

    ThemeProvider::new(theme).child(body).into()
}

pub fn sketch() -> Result<()> {
    Sketch::view("code-blocks", view)
        .viewport(108, 34)
        .write()?;
    Ok(())
}
