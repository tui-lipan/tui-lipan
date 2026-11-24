use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[path = "../format_common.rs"]
mod format_common;
#[path = "../rsx_ast.rs"]
mod rsx_ast;
#[path = "../rsx_format.rs"]
mod rsx_format;
#[path = "../ui_ast.rs"]
mod ui_ast;
#[path = "../ui_format.rs"]
mod ui_format;

use ui_format::format_file_contents;

fn main() {
    if let Err(err) = run() {
        eprintln!("ui-fmt: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut check = false;
    let mut stdin = false;
    let mut paths = Vec::new();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "--stdin" => stdin = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown flag `{arg}`").into());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }

    if stdin {
        if !paths.is_empty() {
            return Err("`--stdin` cannot be combined with file paths".into());
        }

        let mut source = String::new();
        io::stdin().read_to_string(&mut source)?;
        let output = format_file_contents(&source)?.unwrap_or(source);
        io::stdout().write_all(output.as_bytes())?;
        return Ok(());
    }

    if paths.is_empty() {
        paths = collect_rust_files(&env::current_dir()?)?;
    }

    let mut changed = false;

    for path in paths {
        if path.is_dir() {
            for file in collect_rust_files(&path)? {
                let source = fs::read_to_string(&file)?;
                if let Some(formatted) = format_file_contents(&source)? {
                    changed = true;
                    if !check {
                        fs::write(&file, formatted)?;
                    }
                }
            }
            continue;
        }

        let source = fs::read_to_string(&path)?;
        if let Some(formatted) = format_file_contents(&source)? {
            changed = true;
            if !check {
                fs::write(&path, formatted)?;
            }
        }
    }

    if check && changed {
        return Err("some ui! macros need formatting".into());
    }

    Ok(())
}

fn print_help() {
    println!(
        "Usage: ui-fmt [--check] [files...]\n       ui-fmt --stdin\n\nFormats ui! macro bodies inside Rust source files. When no files are provided, all Rust files under the current directory are formatted."
    );
}

fn collect_rust_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_rust_files_recursive(root, &mut paths)?;
    Ok(paths)
}

fn collect_rust_files_recursive(dir: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }

            collect_rust_files_recursive(&path, paths)?;
            continue;
        }

        if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            paths.push(path);
        }
    }

    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') || name == "target")
}
