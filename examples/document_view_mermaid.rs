//! DocumentView Mermaid fenced-block rendering.
//!
//! Run with: cargo run --example document_view_mermaid --features markdown

#[cfg(feature = "markdown")]
use tui_lipan::prelude::*;

#[cfg(feature = "markdown")]
struct MermaidDoc;

#[cfg(feature = "markdown")]
impl Component for MermaidDoc {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        DocumentView::new(MERMAID_MARKDOWN)
            .markdown()
            .wrap(false)
            .line_numbers(true)
            .h_scrollbar(true)
            .scrollbar(true)
            .into()
    }
}

#[cfg(feature = "markdown")]
fn main() -> Result<()> {
    App::new()
        .title("DocumentView Mermaid")
        .mount(MermaidDoc)
        .run()
}

#[cfg(not(feature = "markdown"))]
fn main() {
    eprintln!("Run with: cargo run --example document_view_mermaid --features markdown");
}

#[cfg(feature = "markdown")]
const MERMAID_MARKDOWN: &str = r#"# Mermaid in DocumentView

```mermaid
flowchart TD
    A[Start] --> B{Ready?}
    B -->|yes| C[Render]
```

```mermaid
sequenceDiagram
    participant U as User
    participant A as App
    U->>A: open docs
    Note over A: Markdown formatter parses the fence
    A-->>U: render preview
```

```mermaid
classDiagram
    class DocumentView {
        +format()
        +flatten()
    }
    DocumentView <|-- MarkdownView
```

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Rendering: markdown changed
    Rendering --> Idle
```

```mermaid
erDiagram
    USER ||--o{ NOTE : owns
    USER {
        string id PK
        string name
    }
```

```mermaid
pie
    title Work split
    "Parsing" : 35
    "Rendering" : 45
    "Copy" : 20
```

```mermaid
gantt
   title Sample Schedule
   dateFormat  YYYY-MM-DD
   section Build
   Design        :a1, 2026-05-01, 3d
   Implement     :a2, after a1, 4d
   Test          :a3, after a2, 2d
   Release       :milestone, a4, after a3, 0d
```
"#;
