use crate::archive_source;
use crate::git_source;
use crate::source_db::{ImportDisposition, LoadedSource, SourceDatabase, SourceOrigin};

#[derive(Debug)]
pub struct CompileRequest {
    pub source: LoadedSource,
    pub entry: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug)]
pub enum Outcome {
    Complete,
    Compile(CompileRequest),
}

enum Command {
    ImportArchive {
        project: String,
        archive: String,
    },
    ImportGit {
        project: Option<String>,
        link: String,
        commit: Option<String>,
    },
    List {
        project: Option<String>,
    },
    DeleteRevision {
        project: String,
        revision: u64,
    },
    DeleteProject {
        project: String,
    },
    ExportArchive {
        project: String,
        revision: Option<u64>,
        archive: String,
    },
    Compile {
        project: String,
        revision: Option<u64>,
        entry: Option<String>,
        output: Option<String>,
    },
    Verify,
    Help,
}

pub fn is_source_command(args: &[String]) -> bool {
    args.get(1).is_some_and(|arg| arg == "source")
}

pub async fn execute(args: &[String]) -> Result<Outcome, String> {
    let command = parse(args)?;
    match command {
        Command::Help => {
            print_help();
            Ok(Outcome::Complete)
        }
        Command::Verify => {
            let database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            database.verify().map_err(|error| error.to_string())?;
            println!(
                "TRUEOS rustc source: database verified path={}",
                database.path().display()
            );
            Ok(Outcome::Complete)
        }
        Command::List { project } => {
            let database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            if let Some(project) = project {
                let revisions = database
                    .list_revisions(project.as_str())
                    .map_err(|error| error.to_string())?;
                if revisions.is_empty() {
                    println!("TRUEOS rustc source: no revisions for project={project}");
                }
                for revision in revisions {
                    print_revision(&revision);
                }
            } else {
                let projects = database
                    .list_projects()
                    .map_err(|error| error.to_string())?;
                if projects.is_empty() {
                    println!("TRUEOS rustc source: database is empty");
                }
                for project in projects {
                    println!(
                        "project={} revisions={} latest={}",
                        project.name,
                        project.revision_count,
                        project
                            .latest_revision
                            .map_or_else(|| String::from("-"), |revision| revision.to_string())
                    );
                }
            }
            Ok(Outcome::Complete)
        }
        Command::ImportArchive { project, archive } => {
            let files = archive_source::read_archive(archive.as_str()).await?;
            let mut database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            let imported = database
                .import_local(project.as_str(), Some(archive.as_str()), files)
                .map_err(|error| error.to_string())?;
            print_import(imported.disposition, &imported.revision);
            Ok(Outcome::Complete)
        }
        Command::ImportGit {
            project,
            link,
            commit,
        } => {
            let reference = git_source::parse_commit_link(link.as_str(), commit.as_deref())
                .map_err(|error| error.to_string())?;
            let mut database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            if let Some(existing) = database
                .find_git(reference.repository.as_str(), reference.commit.as_str())
                .map_err(|error| error.to_string())?
            {
                print_import(ImportDisposition::AlreadyPresent, &existing);
                return Ok(Outcome::Complete);
            }
            let files =
                git_source::fetch_commit_files(&reference).map_err(|error| error.to_string())?;
            let project = project.unwrap_or(reference.project_name);
            let imported = database
                .import_git(
                    project.as_str(),
                    reference.repository.as_str(),
                    reference.commit.as_str(),
                    files,
                )
                .map_err(|error| error.to_string())?;
            print_import(imported.disposition, &imported.revision);
            Ok(Outcome::Complete)
        }
        Command::DeleteRevision { project, revision } => {
            let mut database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            if !database
                .delete_revision(project.as_str(), revision)
                .map_err(|error| error.to_string())?
            {
                return Err(format!("project {project} has no revision {revision}"));
            }
            println!("TRUEOS rustc source: deleted project={project} revision={revision}");
            Ok(Outcome::Complete)
        }
        Command::DeleteProject { project } => {
            let mut database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            if !database
                .delete_project(project.as_str())
                .map_err(|error| error.to_string())?
            {
                return Err(format!("project {project} does not exist"));
            }
            println!("TRUEOS rustc source: deleted project={project} all-revisions=true");
            Ok(Outcome::Complete)
        }
        Command::ExportArchive {
            project,
            revision,
            archive,
        } => {
            let database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            let source = database
                .get(project.as_str(), revision)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| revision_not_found(project.as_str(), revision))?;
            let report = archive_source::write_archive(&source, archive.as_str()).await?;
            println!(
                "TRUEOS rustc source: exported project={} revision={} files={} source-bytes={} archive-bytes={} path={}",
                source.revision.project,
                source.revision.revision,
                report.file_count,
                report.input_bytes,
                report.output_bytes,
                archive
            );
            Ok(Outcome::Complete)
        }
        Command::Compile {
            project,
            revision,
            entry,
            output,
        } => {
            let database = SourceDatabase::open_default().map_err(|error| error.to_string())?;
            let source = database
                .get(project.as_str(), revision)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| revision_not_found(project.as_str(), revision))?;
            Ok(Outcome::Compile(CompileRequest {
                source,
                entry,
                output,
            }))
        }
    }
}

fn parse(args: &[String]) -> Result<Command, String> {
    let command = args.get(2).map(String::as_str).unwrap_or("help");
    let tail = args.get(3..).unwrap_or_default();
    match command {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "verify" if tail.is_empty() => Ok(Command::Verify),
        "list" if tail.len() <= 1 => Ok(Command::List {
            project: tail.first().cloned(),
        }),
        "import-7z" if tail.len() == 2 => {
            require_suffix(tail[1].as_str(), ".7z", "source archive")?;
            Ok(Command::ImportArchive {
                project: tail[0].clone(),
                archive: tail[1].clone(),
            })
        }
        "import-git" if (1..=3).contains(&tail.len()) => {
            let (project, link, commit) = match tail {
                [link] => (None, link.clone(), None),
                [link, commit] if link.starts_with("https://") => {
                    (None, link.clone(), Some(commit.clone()))
                }
                [project, link] => (Some(project.clone()), link.clone(), None),
                [project, link, commit] => {
                    (Some(project.clone()), link.clone(), Some(commit.clone()))
                }
                _ => unreachable!(),
            };
            Ok(Command::ImportGit {
                project,
                link,
                commit,
            })
        }
        "delete" if tail.len() == 2 && tail[1] == "--all" => Ok(Command::DeleteProject {
            project: tail[0].clone(),
        }),
        "delete" if tail.len() == 2 => Ok(Command::DeleteRevision {
            project: tail[0].clone(),
            revision: parse_revision(tail[1].as_str())?
                .ok_or_else(|| String::from("delete requires a numeric revision"))?,
        }),
        "export-7z" if tail.len() == 3 => {
            require_suffix(tail[2].as_str(), ".7z", "source archive")?;
            Ok(Command::ExportArchive {
                project: tail[0].clone(),
                revision: parse_revision(tail[1].as_str())?,
                archive: tail[2].clone(),
            })
        }
        "compile" if !tail.is_empty() => parse_compile(tail),
        _ => Err(format!(
            "bad source command; run `rustc-min source help` (command={command})"
        )),
    }
}

fn parse_compile(args: &[String]) -> Result<Command, String> {
    let project = args[0].clone();
    let mut revision = None;
    let mut entry = None;
    let mut output = None;
    let mut index = 1usize;
    if let Some(value) = args.get(index).filter(|value| !value.starts_with("--")) {
        revision = parse_revision(value.as_str())?;
        index += 1;
    }
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?
            .clone();
        match flag {
            "--entry" if entry.is_none() => entry = Some(value),
            "--output" if output.is_none() => output = Some(value),
            _ => return Err(format!("unsupported or duplicate compile option `{flag}`")),
        }
        index += 2;
    }
    Ok(Command::Compile {
        project,
        revision,
        entry,
        output,
    })
}

fn parse_revision(value: &str) -> Result<Option<u64>, String> {
    if value == "latest" {
        Ok(None)
    } else {
        value
            .parse::<u64>()
            .ok()
            .filter(|revision| *revision != 0)
            .map(Some)
            .ok_or_else(|| format!("invalid revision `{value}`"))
    }
}

fn require_suffix(path: &str, suffix: &str, label: &str) -> Result<(), String> {
    if path.len() > suffix.len()
        && path
            .get(path.len() - suffix.len()..)
            .is_some_and(|actual| actual.eq_ignore_ascii_case(suffix))
    {
        Ok(())
    } else {
        Err(format!("{label} must end in `{suffix}`: {path}"))
    }
}

fn revision_not_found(project: &str, revision: Option<u64>) -> String {
    revision.map_or_else(
        || format!("project {project} has no revisions"),
        |revision| format!("project {project} has no revision {revision}"),
    )
}

fn print_import(disposition: ImportDisposition, revision: &crate::source_db::RevisionSummary) {
    let action = match disposition {
        ImportDisposition::Inserted => "inserted",
        ImportDisposition::AlreadyPresent => "already-present",
    };
    print!("TRUEOS rustc source: {action} ");
    print_revision(revision);
}

fn print_revision(revision: &crate::source_db::RevisionSummary) {
    let origin = match &revision.origin {
        SourceOrigin::Local { label } => {
            format!("local:{}", label.as_deref().unwrap_or("<unlabelled>"))
        }
        SourceOrigin::Git { repository, commit } => format!("git:{repository}@{commit}"),
    };
    println!(
        "project={} revision={} id={} files={} bytes={} origin={origin}",
        revision.project,
        revision.revision,
        revision.id,
        revision.file_count,
        revision.source_bytes
    );
}

fn print_help() {
    println!(
        "rustc-min immutable source database:
  rustc-min source import-7z <project> <archive.7z>
  rustc-min source import-git [project] <github-commit-link> [exact-commit]
  rustc-min source list [project]
  rustc-min source delete <project> <revision>
  rustc-min source delete <project> --all
  rustc-min source export-7z <project> <revision|latest> <archive.7z>
  rustc-min source compile <project> [revision|latest] [--entry path] [--output path.bp]
  rustc-min source verify"
    );
}

#[cfg(test)]
mod tests {
    use super::{Command, parse};

    fn args(tail: &[&str]) -> Vec<String> {
        ["rustc-min", "source"]
            .into_iter()
            .chain(tail.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn compile_options_are_explicit() {
        let command = parse(&args(&[
            "compile",
            "demo",
            "3",
            "--entry",
            "src/bin/tool.rs",
            "--output",
            "/common/localcompile/tool.bp",
        ]))
        .unwrap();
        let Command::Compile {
            project,
            revision,
            entry,
            output,
        } = command
        else {
            panic!("wrong command");
        };
        assert_eq!(project, "demo");
        assert_eq!(revision, Some(3));
        assert_eq!(entry.as_deref(), Some("src/bin/tool.rs"));
        assert_eq!(output.as_deref(), Some("/common/localcompile/tool.bp"));
    }

    #[test]
    fn git_repository_plus_commit_does_not_require_a_project_override() {
        let command = parse(&args(&[
            "import-git",
            "https://github.com/t4ce/TRUEOS",
            "0123456789abcdef0123456789abcdef01234567",
        ]))
        .unwrap();
        let Command::ImportGit {
            project,
            link,
            commit,
        } = command
        else {
            panic!("wrong command");
        };
        assert_eq!(project, None);
        assert_eq!(link, "https://github.com/t4ce/TRUEOS");
        assert_eq!(
            commit.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }

    #[test]
    fn archive_commands_require_7z_destinations() {
        assert!(parse(&args(&["import-7z", "demo", "source.sqlite3"])).is_err());
        assert!(parse(&args(&["export-7z", "demo", "latest", "source.sqlite3"])).is_err());
        assert!(parse(&args(&["import-7z", "demo", "source.7z"])).is_ok());
    }
}
