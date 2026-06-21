use serde::Deserialize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io_string;

#[derive(Default)]
pub(crate) struct CargoBuildArtifacts {
    pub(crate) rlibs: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct CargoJsonMessage {
    reason: String,
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    message: Option<CargoJsonDiagnostic>,
}

#[derive(Deserialize)]
struct CargoJsonDiagnostic {
    rendered: Option<String>,
}

#[derive(Default)]
pub(crate) struct CargoOutputNotes {
    unused_patch_diagnostics: usize,
    build_std_future_incompat: usize,
}

pub(crate) fn run_cargo_command(cmd: &mut Command, label: &str) -> Result<(), String> {
    let output = cmd
        .output()
        .map_err(|err| format!("{label} failed to start: {err}"))?;
    let notes = write_filtered_cargo_output(label, &output.stdout, &output.stderr)?;
    print_cargo_output_notes(label, &notes);
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with status {}", output.status))
    }
}

pub(crate) fn run_cargo_rustc_command(
    cmd: &mut Command,
    label: &str,
    deps_dir: &Path,
) -> Result<CargoBuildArtifacts, String> {
    let output = cmd
        .output()
        .map_err(|err| format!("{label} failed to start: {err}"))?;

    let mut artifacts = CargoBuildArtifacts::default();
    let mut rendered_stdout = String::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        match serde_json::from_str::<CargoJsonMessage>(line) {
            Ok(message) => match message.reason.as_str() {
                "compiler-artifact" => {
                    for filename in message.filenames {
                        let path = PathBuf::from(filename);
                        if path.extension().and_then(|ext| ext.to_str()) != Some("rlib") {
                            continue;
                        }
                        if !path.parent().is_some_and(|parent| parent == deps_dir) {
                            continue;
                        }
                        if !artifacts.rlibs.iter().any(|existing| existing == &path) {
                            artifacts.rlibs.push(path);
                        }
                    }
                }
                "compiler-message" => {
                    if let Some(rendered) = message.message.and_then(|message| message.rendered) {
                        rendered_stdout.push_str(&rendered);
                        if !rendered.ends_with('\n') {
                            rendered_stdout.push('\n');
                        }
                    }
                }
                _ => {}
            },
            Err(_) => {
                rendered_stdout.push_str(line);
                rendered_stdout.push('\n');
            }
        }
    }

    io::stdout()
        .write_all(rendered_stdout.as_bytes())
        .map_err(io_string)?;
    let notes = write_filtered_cargo_output(label, &[], &output.stderr)?;
    print_cargo_output_notes(label, &notes);

    if output.status.success() {
        Ok(artifacts)
    } else {
        Err(format!("{label} failed with status {}", output.status))
    }
}

pub(crate) fn write_filtered_cargo_output(
    label: &str,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<CargoOutputNotes, String> {
    io::stdout().write_all(stdout).map_err(io_string)?;
    let stderr = String::from_utf8_lossy(stderr);
    let mut filtered = String::with_capacity(stderr.len());
    let mut notes = CargoOutputNotes::default();
    let mut skip_patch_help = false;
    let mut skip_future_incompat_note = false;

    for line in stderr.lines() {
        if line.starts_with("warning: patch `")
            && line.ends_with("` was not used in the crate graph")
        {
            notes.unused_patch_diagnostics += 1;
            skip_patch_help = true;
            continue;
        }

        if line.starts_with("warning: the following packages contain code that will be rejected by a future version of Rust: std v0.0.0 ")
        {
            notes.build_std_future_incompat += 1;
            skip_future_incompat_note = true;
            continue;
        }

        if skip_patch_help && line.starts_with("help: Check that the patched package version") {
            continue;
        }
        if skip_patch_help
            && (line.starts_with("      with the dependency requirements.")
                || line
                    .starts_with("      what is locked in the Cargo.lock file, run `cargo update`")
                || line
                    .starts_with("      version. This may also occur with an optional dependency"))
        {
            continue;
        }

        if skip_future_incompat_note
            && (line.starts_with("note: to see what the problems were, use the option")
                || line.starts_with("or run `cargo report future-incompatibilities"))
        {
            continue;
        }

        skip_patch_help = false;
        skip_future_incompat_note = false;
        filtered.push_str(line);
        filtered.push('\n');
    }

    io::stderr()
        .write_all(filtered.as_bytes())
        .map_err(|err| format!("{label} output write failed: {err}"))?;
    Ok(notes)
}

pub(crate) fn print_cargo_output_notes(label: &str, notes: &CargoOutputNotes) {
    if notes.unused_patch_diagnostics != 0 {
        eprintln!(
            "trueos-blueprint: note: suppressed {} unused source-overlay patch diagnostics during {label}",
            notes.unused_patch_diagnostics
        );
    }
    if notes.build_std_future_incompat != 0 {
        eprintln!(
            "trueos-blueprint: note: suppressed {} build-std future-incompat report for synthetic std during {label}",
            notes.build_std_future_incompat
        );
    }
}
