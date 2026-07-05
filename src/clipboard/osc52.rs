use std::io::Write;

fn is_passthrough_needed() -> bool {
    std::env::var("TMUX").is_ok() || std::env::var("STY").is_ok()
}

pub(crate) fn write_osc52(text: &str) {
    if cfg!(test) {
        return;
    }
    use base64::{Engine as _, engine::general_purpose};

    let b64 = general_purpose::STANDARD.encode(text);
    let osc52 = format!("\x1b]52;c;{}\x07", b64);

    let sequence = if is_passthrough_needed() {
        format!("\x1bPtmux;\x1b{}\x1b\\", osc52)
    } else {
        osc52
    };

    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(sequence.as_bytes());
    let _ = stdout.flush();
}
