// src/output.rs
//! Planned outputs and protected filesystem replacement.

// Output imports
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::parser::SourceOrigin;

// Planned output model
/// A complete file proposed by a Litweb output backend.
///
/// `relative_path` must stay beneath the output directory supplied to the
/// writer: it cannot be empty, absolute, prefixed, or contain `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutput {
    /// The file's path relative to the selected output directory.
    pub relative_path: PathBuf,
    /// The complete bytes to write.
    pub bytes: Vec<u8>,
    /// The source request responsible for the file.
    pub origin: SourceOrigin,
}

// Preflighted output model
struct PreflightedOutput<'a> {
    output: &'a PlannedOutput,
    destination: PathBuf,
    absolute_destination: PathBuf,
    existing_permissions: Option<fs::Permissions>,
}

// Output diagnostics
/// A filesystem operation that can fail while installing planned output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAction {
    ResolveCurrentDirectory,
    InspectDestination,
    CreateDirectory,
    CreateTemporaryFile,
    WriteTemporaryFile,
    PreservePermissions,
    ReplaceDestination,
}

impl fmt::Display for OutputAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResolveCurrentDirectory => f.write_str("resolve the current directory"),
            Self::InspectDestination => f.write_str("inspect the destination"),
            Self::CreateDirectory => f.write_str("create an output directory"),
            Self::CreateTemporaryFile => f.write_str("create a temporary output file"),
            Self::WriteTemporaryFile => f.write_str("write a temporary output file"),
            Self::PreservePermissions => f.write_str("preserve destination permissions"),
            Self::ReplaceDestination => f.write_str("replace the destination"),
        }
    }
}

/// A filesystem or path conflict associated with one requested output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputError {
    /// The source request responsible for the output.
    pub origin: SourceOrigin,
    /// The rejected relative path or affected joined destination.
    pub path: PathBuf,
    /// The structured reason writing could not continue.
    pub kind: OutputErrorKind,
}

/// Why a planned relative path cannot be confined to the output directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidOutputPathReason {
    Empty,
    Absolute,
    PlatformPrefix,
    ParentTraversal,
}

impl fmt::Display for InvalidOutputPathReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the path is empty"),
            Self::Absolute => f.write_str("the path is absolute"),
            Self::PlatformPrefix => f.write_str("the path has a platform prefix"),
            Self::ParentTraversal => {
                f.write_str("the path contains a parent-directory component")
            }
        }
    }
}

/// The structured reason an output could not be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputErrorKind {
    InvalidRelativePath {
        reason: InvalidOutputPathReason,
    },
    InputCollision,
    DuplicateDestination,
    ParentIsPlannedFile,
    DestinationIsDirectory,
    ParentIsNotDirectory,
    Io {
        action: OutputAction,
        error: io::ErrorKind,
    },
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: error: output {}: ",
            self.origin,
            self.path.display()
        )?;
        match self.kind {
            OutputErrorKind::InvalidRelativePath { reason } => {
                write!(f, "invalid planned relative path: {reason}")
            }
            OutputErrorKind::InputCollision => {
                f.write_str("the output would overwrite the input file")
            }
            OutputErrorKind::DuplicateDestination => {
                f.write_str("more than one generated file has this destination")
            }
            OutputErrorKind::ParentIsPlannedFile => f.write_str(
                "another generated file occupies a parent directory of this output",
            ),
            OutputErrorKind::DestinationIsDirectory => {
                f.write_str("the destination is an existing directory")
            }
            OutputErrorKind::ParentIsNotDirectory => {
                f.write_str("an output parent is not a directory")
            }
            OutputErrorKind::Io { action, error } => {
                write!(f, "could not {action}: {error}")
            }
        }
    }
}

impl std::error::Error for OutputError {}

// Protected output writer
/// Writes planned files beneath `output_directory` after checking the whole plan.
///
/// Predictable conflicts and unconfined relative paths fail before filesystem
/// mutation. Each existing regular destination keeps its standard filesystem
/// permissions. Replacement is atomic per file where the platform supports a
/// replacing rename, but the complete output list is not a transaction.
pub fn write_planned_outputs_protecting(
    outputs: &[PlannedOutput],
    output_directory: &Path,
    inputs: &[PathBuf],
) -> Result<(), OutputError> {
    // Preflight every planned destination
    let mut preflighted = Vec::with_capacity(outputs.len());
    let mut seen = HashSet::with_capacity(outputs.len());
    for output in outputs {
        validate_relative_path(&output.relative_path).map_err(|reason| OutputError {
            origin: output.origin.clone(),
            path: output.relative_path.clone(),
            kind: OutputErrorKind::InvalidRelativePath { reason },
        })?;
        let destination = output_directory.join(&output.relative_path);
        let absolute_destination =
            lexical_absolute(&destination).map_err(|error| OutputError {
                origin: output.origin.clone(),
                path: destination.clone(),
                kind: OutputErrorKind::Io {
                    action: OutputAction::ResolveCurrentDirectory,
                    error: error.kind(),
                },
            })?;
        let existing_permissions = preflight_destination(
            &destination,
            &absolute_destination,
            inputs,
            &output.origin,
        )?;
        if !seen.insert(absolute_destination.clone()) {
            return Err(OutputError {
                origin: output.origin.clone(),
                path: destination,
                kind: OutputErrorKind::DuplicateDestination,
            });
        }
        preflighted.push(PreflightedOutput {
            output,
            destination,
            absolute_destination,
            existing_permissions,
        });
    }

    for item in &preflighted {
        if item
            .absolute_destination
            .ancestors()
            .skip(1)
            .any(|parent| seen.contains(parent))
        {
            return Err(OutputError {
                origin: item.output.origin.clone(),
                path: item.destination.clone(),
                kind: OutputErrorKind::ParentIsPlannedFile,
            });
        }
    }

    // Create directories and replace planned files
    for item in preflighted {
        if let Some(parent) = item.destination.parent() {
            fs::create_dir_all(parent).map_err(|error| OutputError {
                origin: item.output.origin.clone(),
                path: item.destination.clone(),
                kind: OutputErrorKind::Io {
                    action: OutputAction::CreateDirectory,
                    error: error.kind(),
                },
            })?;
        }
        replace_file(
            item.output,
            &item.destination,
            item.existing_permissions.as_ref(),
        )?;
    }
    Ok(())
}

// Single-input writer entry point
/// Writes planned files while protecting one input path from replacement.
pub fn write_planned_outputs(
    outputs: &[PlannedOutput],
    output_directory: &Path,
    input: &Path,
) -> Result<(), OutputError> {
    write_planned_outputs_protecting(outputs, output_directory, &[input.to_path_buf()])
}

// Validate a planned relative path
fn validate_relative_path(path: &Path) -> Result<(), InvalidOutputPathReason> {
    if path.is_absolute() {
        return Err(InvalidOutputPathReason::Absolute);
    }

    let mut has_normal_component = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_normal_component = true,
            Component::CurDir => {}
            Component::ParentDir => return Err(InvalidOutputPathReason::ParentTraversal),
            Component::RootDir => return Err(InvalidOutputPathReason::Absolute),
            Component::Prefix(_) => return Err(InvalidOutputPathReason::PlatformPrefix),
        }
    }

    if has_normal_component {
        Ok(())
    } else {
        Err(InvalidOutputPathReason::Empty)
    }
}

// Preflight one destination
fn preflight_destination(
    destination: &Path,
    destination_absolute: &Path,
    inputs: &[PathBuf],
    origin: &SourceOrigin,
) -> Result<Option<fs::Permissions>, OutputError> {
    for input in inputs {
        let input_absolute = lexical_absolute(input).map_err(|error| OutputError {
            origin: origin.clone(),
            path: destination.to_owned(),
            kind: OutputErrorKind::Io {
                action: OutputAction::ResolveCurrentDirectory,
                error: error.kind(),
            },
        })?;

        let canonical_collision = if destination.exists() && input.exists() {
            match (fs::canonicalize(destination), fs::canonicalize(input)) {
                (Ok(destination), Ok(input)) => destination == input,
                _ => false,
            }
        } else {
            false
        };
        if destination_absolute == input_absolute || canonical_collision {
            return Err(OutputError {
                origin: origin.clone(),
                path: destination.to_owned(),
                kind: OutputErrorKind::InputCollision,
            });
        }
    }

    let existing_permissions = match fs::metadata(destination) {
        Ok(metadata) if metadata.is_dir() => {
            return Err(OutputError {
                origin: origin.clone(),
                path: destination.to_owned(),
                kind: OutputErrorKind::DestinationIsDirectory,
            });
        }
        Ok(metadata) if metadata.is_file() => {
            let permissions = metadata.permissions();
            #[cfg(windows)]
            if permissions.readonly() {
                return Err(OutputError {
                    origin: origin.clone(),
                    path: destination.to_owned(),
                    kind: OutputErrorKind::Io {
                        action: OutputAction::ReplaceDestination,
                        error: io::ErrorKind::PermissionDenied,
                    },
                });
            }
            Some(permissions)
        }
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
            return Err(OutputError {
                origin: origin.clone(),
                path: destination.to_owned(),
                kind: OutputErrorKind::ParentIsNotDirectory,
            });
        }
        Err(error) => {
            return Err(OutputError {
                origin: origin.clone(),
                path: destination.to_owned(),
                kind: OutputErrorKind::Io {
                    action: OutputAction::InspectDestination,
                    error: error.kind(),
                },
            });
        }
    };

    let mut ancestor = destination.parent();
    while let Some(path) = ancestor {
        match fs::metadata(path) {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(OutputError {
                    origin: origin.clone(),
                    path: destination.to_owned(),
                    kind: OutputErrorKind::ParentIsNotDirectory,
                });
            }
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = path.parent();
            }
            Err(error) => {
                return Err(OutputError {
                    origin: origin.clone(),
                    path: destination.to_owned(),
                    kind: OutputErrorKind::Io {
                        action: OutputAction::InspectDestination,
                        error: error.kind(),
                    },
                });
            }
        }
    }
    Ok(existing_permissions)
}

// Compute a lexical absolute path
fn lexical_absolute(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

// Temporary output machinery
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn create_temporary(
    destination: &Path,
    origin: &SourceOrigin,
) -> Result<(File, PathBuf), OutputError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let filename = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    loop {
        let number = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{filename}.lw-{}-{number}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((file, temporary_path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(OutputError {
                    origin: origin.clone(),
                    path: destination.to_owned(),
                    kind: OutputErrorKind::Io {
                        action: OutputAction::CreateTemporaryFile,
                        error: error.kind(),
                    },
                });
            }
        }
    }
}

// Replace one output file
fn replace_file(
    output: &PlannedOutput,
    destination: &Path,
    existing_permissions: Option<&fs::Permissions>,
) -> Result<(), OutputError> {
    let (mut temporary, temporary_path) = create_temporary(destination, &output.origin)?;
    if let Err(error) = temporary
        .write_all(&output.bytes)
        .and_then(|()| temporary.flush())
    {
        drop(temporary);
        let _ = fs::remove_file(&temporary_path);
        return Err(OutputError {
            origin: output.origin.clone(),
            path: destination.to_owned(),
            kind: OutputErrorKind::Io {
                action: OutputAction::WriteTemporaryFile,
                error: error.kind(),
            },
        });
    }
    let permission_result = match existing_permissions {
        Some(permissions) => temporary.set_permissions(permissions.clone()),
        None => Ok(()),
    };
    if let Err(error) = permission_result {
        drop(temporary);
        let _ = fs::remove_file(&temporary_path);
        return Err(OutputError {
            origin: output.origin.clone(),
            path: destination.to_owned(),
            kind: OutputErrorKind::Io {
                action: OutputAction::PreservePermissions,
                error: error.kind(),
            },
        });
    }
    drop(temporary);

    #[cfg(windows)]
    if destination.exists() {
        if let Err(error) = fs::remove_file(destination) {
            let _ = fs::remove_file(&temporary_path);
            return Err(OutputError {
                origin: output.origin.clone(),
                path: destination.to_owned(),
                kind: OutputErrorKind::Io {
                    action: OutputAction::ReplaceDestination,
                    error: error.kind(),
                },
            });
        }
    }

    if let Err(error) = fs::rename(&temporary_path, destination) {
        let _ = fs::remove_file(&temporary_path);
        return Err(OutputError {
            origin: output.origin.clone(),
            path: destination.to_owned(),
            kind: OutputErrorKind::Io {
                action: OutputAction::ReplaceDestination,
                error: error.kind(),
            },
        });
    }
    Ok(())
}
