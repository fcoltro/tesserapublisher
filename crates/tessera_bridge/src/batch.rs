//! Tessera in a script: convert a document and preflight it from the
//! command line, with no window.
//!
//! ```text
//! tessera_app --convert IN OUT [--standard plain|x1a|x4]
//! tessera_app --preflight IN
//! ```
//!
//! `IN` is a `.tsrdf` or an `.idml`. `OUT` is written by its extension: PDF,
//! IDML, HTML, EPUB or `.tsrdf`. `--preflight` prints the report as JSON and
//! exits 1 when it holds an error, so a build can stop on a job that would
//! not print.
//!
//! The same tools a model calls over MCP do the work — [`crate::tools::call`]
//! — so a script and a model cannot get different answers from one document.
//! DesignCraft's `designcraft-cli run` is the idea this follows.

use std::path::Path;

use serde_json::{Value, json};
use tessera_ui::TesseraApp;

use crate::Failure;

/// The usage, for `--help` and for arguments that make no sense.
pub const USAGE: &str = "usage: tessera_app --convert IN OUT [--standard plain|x1a|x4]\n       \
                         tessera_app --preflight IN\n       \
                         tessera_app --mcp\n       \
                         tessera_app [FILE...]";

/// Run a batch invocation and say how the process should exit, or `None`
/// when `args` (without the program's name) ask for the window instead.
pub fn run(args: &[String]) -> Option<i32> {
    let first = args.first()?.as_str();
    let outcome = match first {
        "--convert" => match (args.get(1), args.get(2)) {
            (Some(input), Some(output)) => {
                let standard = flag_value(&args[3..], "--standard");
                convert(Path::new(input), Path::new(output), standard.as_deref())
                    .map(|said| (said, 0))
            }
            _ => Err(USAGE.to_owned()),
        },
        "--preflight" => match args.get(1) {
            Some(input) => preflight(Path::new(input)).map(|report| {
                let errors = report["errors"].as_u64().unwrap_or(0);
                (report.to_string(), i32::from(errors > 0))
            }),
            None => Err(USAGE.to_owned()),
        },
        "--help" | "-h" => Ok((USAGE.to_owned(), 0)),
        _ => return None,
    };
    Some(match outcome {
        Ok((said, code)) => {
            println!("{said}");
            code
        }
        Err(why) => {
            eprintln!("tessera: {why}");
            2
        }
    })
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// A headless application with `input` open in it.
fn opened(input: &Path) -> Result<TesseraApp, String> {
    let mut state = TesseraApp::headless();
    if tessera_ui::file_ops::is_idml(input) {
        tessera_ui::file_ops::import_idml(&mut state, input).map_err(|e| e.to_string())?;
    } else {
        tessera_ui::file_ops::open_from_path(&mut state, input).map_err(|e| e.to_string())?;
    }
    Ok(state)
}

fn tool(state: &mut TesseraApp, name: &str, arguments: Value) -> Result<Value, String> {
    crate::tools::call(state, name, &arguments).map_err(|f| match f {
        Failure::NoSuchTool => format!("no tool named {name}"),
        Failure::Refused(why) => why,
    })
}

/// Open `input` and write it to `output`, in the format its extension
/// names. Says what was written, and what could not be carried.
pub fn convert(input: &Path, output: &Path, standard: Option<&str>) -> Result<String, String> {
    let mut state = opened(input)?;
    let extension = output
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => {
            let mut arguments = json!({ "path": output });
            if let Some(standard) = standard {
                arguments["standard"] = json!(standard);
            }
            tool(&mut state, "export_pdf", arguments)?;
        }
        "idml" => {
            let written = tessera_import::idml::write::write(state.active().document())?;
            tessera_io::atomic::write_atomic(output, &written.bytes).map_err(|e| e.to_string())?;
            if !written.dropped.is_empty() {
                return Ok(format!(
                    "wrote {}; not carried: {}",
                    output.display(),
                    written.dropped.0.join("; ")
                ));
            }
        }
        "html" | "htm" => tessera_ui::file_ops::export_html_to(&mut state, output)?,
        "epub" => tessera_ui::file_ops::export_epub_to(&mut state, output)?,
        "tsrdf" => {
            tessera_ui::file_ops::save_to_path(&mut state, output).map_err(|e| e.to_string())?
        }
        other => {
            return Err(format!(
                "cannot write {:?}: give a .pdf, .idml, .html, .epub or .tsrdf",
                other
            ));
        }
    }
    Ok(format!("wrote {}", output.display()))
}

/// Open `input` and return its preflight report, as the `preflight` tool
/// gives it.
pub fn preflight(input: &Path) -> Result<Value, String> {
    let mut state = opened(input)?;
    tool(&mut state, "preflight", json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_saved_document_converts_to_pdf_and_idml_and_preflights() {
        let dir = std::env::temp_dir().join(format!("tessera-batch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("job.tsrdf");
        let mut bridge = crate::Bridge::new();
        crate::tools::call(
            &mut bridge.state,
            "add_rectangle",
            &json!({ "x": 10, "y": 10, "width": 50, "height": 50 }),
        )
        .unwrap();
        crate::tools::call(&mut bridge.state, "save", &json!({ "path": source })).unwrap();

        let pdf = dir.join("job.pdf");
        assert_eq!(
            run(&args(&[
                "--convert",
                source.to_str().unwrap(),
                pdf.to_str().unwrap(),
                "--standard",
                "plain"
            ])),
            Some(0)
        );
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));

        let idml = dir.join("job.idml");
        convert(&source, &idml, None).unwrap();
        assert!(std::fs::read(&idml).unwrap().starts_with(b"PK"));
        // And back: an IDML is an input too.
        let again = dir.join("again.tsrdf");
        convert(&idml, &again, None).unwrap();
        assert!(again.exists());

        let report = preflight(&source).unwrap();
        assert!(report["problems"].is_array());

        assert!(convert(&source, &dir.join("job.xyz"), None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn anything_else_is_left_to_the_window() {
        assert_eq!(run(&args(&[])), None);
        assert_eq!(run(&args(&["some.tsrdf"])), None);
        assert_eq!(run(&args(&["--convert"])), Some(2), "missing paths");
    }
}
