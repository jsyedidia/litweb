use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

const CANONICAL_BOOK: &str = "lit/index.lit";

const EXPECTED_OUTPUTS: &[&str] = &[
    "src/book.rs",
    "src/config.rs",
    "src/identifier.rs",
    "src/inline.rs",
    "src/latex.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/output.rs",
    "src/parser.rs",
    "src/prose.rs",
    "src/resolver.rs",
    "src/tangler.rs",
    "src/util.rs",
    "src/weaver.rs",
    "src/woven.rs",
];

const USAGE: &str = "\
Usage:
    cargo run --locked --offline --example bootstrap -- check
    cargo run --locked --offline --example bootstrap -- stage <empty-directory>";

type Result<T> = std::result::Result<T, BootstrapError>;

#[derive(Debug)]
struct BootstrapError(String);

impl BootstrapError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn io(action: &str, path: &Path, error: io::Error) -> Self {
        Self(format!("{action} {}: {error}", path.display()))
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

enum Action {
    Check,
    Stage(PathBuf),
    Help,
}

fn main() -> ExitCode {
    let action = match parse_arguments(env::args_os().skip(1)) {
        Ok(action) => action,
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    if matches!(action, Action::Help) {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    match run(action) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_arguments(mut arguments: impl Iterator<Item = OsString>) -> Result<Action> {
    let Some(action) = arguments.next() else {
        return Err(BootstrapError::new("missing command"));
    };

    match action.to_str() {
        Some("check") => {
            if arguments.next().is_some() {
                return Err(BootstrapError::new("check takes no arguments"));
            }
            Ok(Action::Check)
        }
        Some("stage") => {
            let destination = arguments
                .next()
                .ok_or_else(|| BootstrapError::new("stage requires a destination"))?;
            if arguments.next().is_some() {
                return Err(BootstrapError::new("stage accepts exactly one destination"));
            }
            Ok(Action::Stage(PathBuf::from(destination)))
        }
        Some("--help" | "-h" | "help") => {
            if arguments.next().is_some() {
                return Err(BootstrapError::new("help takes no arguments"));
            }
            Ok(Action::Help)
        }
        Some(other) => Err(BootstrapError::new(format!("unknown command: {other}"))),
        None => Err(BootstrapError::new("command is not valid UTF-8")),
    }
}

fn run(action: Action) -> Result<()> {
    let repository = repository_root()?;
    validate_committed_tree(&repository)?;

    let cargo = selected_program("CARGO", "cargo");
    let rustfmt = selected_program("RUSTFMT", "rustfmt");

    print_identity("Cargo", &cargo)?;
    print_identity("Rustfmt", &rustfmt)?;

    match action {
        Action::Check => run_check(&repository, &cargo, &rustfmt),
        Action::Stage(destination) => {
            run_stage(&repository, &cargo, &rustfmt, &destination)
        }
        Action::Help => unreachable!("help returns before running"),
    }
}

fn repository_root() -> Result<PathBuf> {
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_directory.canonicalize().map_err(|error| {
        BootstrapError::io("cannot resolve repository root", manifest_directory, error)
    })
}

fn selected_program(environment: &str, fallback: &str) -> OsString {
    env::var_os(environment).unwrap_or_else(|| OsString::from(fallback))
}

fn print_identity(label: &str, program: &OsStr) -> Result<()> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|error| {
            BootstrapError::new(format!(
                "cannot run {} --version: {error}",
                Path::new(program).display()
            ))
        })?;
    if !output.status.success() {
        return Err(BootstrapError::new(format!(
            "{} --version failed with {}",
            Path::new(program).display(),
            output.status
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let identity = stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| !line.trim().is_empty())
        .unwrap_or("version command produced no text");
    println!("{label}: {identity}");
    Ok(())
}

fn run_check(repository: &Path, cargo: &OsStr, rustfmt: &OsStr) -> Result<()> {
    let temporary = OwnedDirectory::new_in(&env::temp_dir(), "litweb-bootstrap-check")?;
    let generated = temporary.path().join("generated");
    generate(repository, cargo, rustfmt, temporary.path(), &generated)?;
    compare_with_committed(repository, &generated)?;
    println!("Canonical Literate Rust reproduces committed src/ exactly.");
    Ok(())
}

fn run_stage(
    repository: &Path,
    cargo: &OsStr,
    rustfmt: &OsStr,
    requested: &Path,
) -> Result<()> {
    let destination = StageDestination::validate(repository, requested)?;
    let parent = destination
        .path
        .parent()
        .expect("validated stage destination has a parent");
    let temporary = OwnedDirectory::new_in(parent, "litweb-bootstrap-stage")?;
    let generated = temporary.path().join("generated");
    generate(repository, cargo, rustfmt, temporary.path(), &generated)?;
    let differences = differing_outputs(repository, &generated)?;
    destination.install(generated)?;
    println!("Staged regenerated Rust at {}.", destination.path.display());
    if differences.is_empty() {
        println!("The staged tree matches committed src/ exactly.");
    } else {
        println!("Staged files that differ from committed src/:");
        for path in differences {
            println!("  {path}");
        }
    }
    Ok(())
}

fn generate(
    repository: &Path,
    cargo: &OsStr,
    rustfmt: &OsStr,
    working_directory: &Path,
    generated: &Path,
) -> Result<()> {
    let target = working_directory.join("target");
    let manifest = repository.join("Cargo.toml");
    let mut build = Command::new(cargo);
    build
        .current_dir(repository)
        .env("CARGO_TARGET_DIR", &target)
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--release")
        .arg("--locked")
        .arg("--offline")
        .arg("--bin")
        .arg("lw");
    run_command("Cargo release build", &mut build)?;

    let executable = target
        .join("release")
        .join(format!("lw{}", env::consts::EXE_SUFFIX));
    if !executable.is_file() {
        return Err(BootstrapError::new(format!(
            "Cargo reported success but did not create {}",
            executable.display()
        )));
    }
    print_identity("lw", executable.as_os_str())?;

    let input_path = repository.join(CANONICAL_BOOK);
    let mut tangle = Command::new(&executable);
    tangle
        .current_dir(repository)
        .arg("-t")
        .arg("-odir")
        .arg(generated)
        .arg(&input_path);
    run_command(&format!("tangle {}", input_path.display()), &mut tangle)?;

    validate_generated_tree(generated)?;
    check_generated_tree(repository, rustfmt, generated)?;
    validate_generated_tree(generated)?;
    Ok(())
}

fn run_command(label: &str, command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .map_err(|error| BootstrapError::new(format!("cannot run {label}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(BootstrapError::new(format!("{label} failed with {status}")))
    }
}

fn check_generated_tree(
    repository: &Path,
    rustfmt: &OsStr,
    generated: &Path,
) -> Result<()> {
    let paths: Vec<PathBuf> = EXPECTED_OUTPUTS
        .iter()
        .map(|path| generated.join(path))
        .collect();

    let mut check = Command::new(rustfmt);
    check
        .arg("--config-path")
        .arg(repository.join("rustfmt.toml"))
        .arg("--edition")
        .arg("2024")
        .arg("--check")
        .args(&paths);
    run_command("Rustfmt generated-source check", &mut check)
}

#[derive(Default)]
struct TreeManifest {
    directories: BTreeSet<PathBuf>,
    files: BTreeSet<PathBuf>,
}

fn expected_manifest(outputs: &[&str]) -> TreeManifest {
    TreeManifest {
        directories: BTreeSet::from([PathBuf::from("src")]),
        files: outputs.iter().map(PathBuf::from).collect(),
    }
}

fn validate_generated_tree(generated: &Path) -> Result<()> {
    validate_tree(generated, EXPECTED_OUTPUTS)
}

fn validate_tree(generated: &Path, outputs: &[&str]) -> Result<()> {
    let actual = scan_tree(generated, generated)?;
    compare_manifests("generated tree", &expected_manifest(outputs), &actual)
}

fn validate_committed_tree(repository: &Path) -> Result<()> {
    let source = repository.join("src");
    let mut actual = scan_tree(repository, &source)?;
    actual.directories.insert(PathBuf::from("src"));
    compare_manifests(
        "committed src/ tree",
        &expected_manifest(EXPECTED_OUTPUTS),
        &actual,
    )
}

fn scan_tree(base: &Path, directory: &Path) -> Result<TreeManifest> {
    if !directory.is_dir() {
        return Err(BootstrapError::new(format!(
            "expected directory does not exist: {}",
            directory.display()
        )));
    }

    let mut manifest = TreeManifest::default();
    scan_directory(base, directory, &mut manifest)?;
    Ok(manifest)
}

fn scan_directory(
    base: &Path,
    directory: &Path,
    manifest: &mut TreeManifest,
) -> Result<()> {
    let entries = fs::read_dir(directory)
        .map_err(|error| BootstrapError::io("cannot read directory", directory, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            BootstrapError::new(format!(
                "cannot read an entry in {}: {error}",
                directory.display()
            ))
        })?;
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .expect("scanned path is below its base")
            .to_path_buf();
        let file_type = entry
            .file_type()
            .map_err(|error| BootstrapError::io("cannot inspect", &path, error))?;
        if file_type.is_dir() {
            manifest.directories.insert(relative);
            scan_directory(base, &path, manifest)?;
        } else if file_type.is_file() {
            manifest.files.insert(relative);
        } else {
            return Err(BootstrapError::new(format!(
                "generated tree contains a non-file entry: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn compare_manifests(
    label: &str,
    expected: &TreeManifest,
    actual: &TreeManifest,
) -> Result<()> {
    let missing_directories: Vec<_> = expected
        .directories
        .difference(&actual.directories)
        .collect();
    let unexpected_directories: Vec<_> = actual
        .directories
        .difference(&expected.directories)
        .collect();
    let missing_files: Vec<_> = expected.files.difference(&actual.files).collect();
    let unexpected_files: Vec<_> = actual.files.difference(&expected.files).collect();

    if missing_directories.is_empty()
        && unexpected_directories.is_empty()
        && missing_files.is_empty()
        && unexpected_files.is_empty()
    {
        return Ok(());
    }

    let mut message = format!("{label} does not match the expected output manifest");
    append_paths(&mut message, "missing directory", &missing_directories);
    append_paths(
        &mut message,
        "unexpected directory",
        &unexpected_directories,
    );
    append_paths(&mut message, "missing file", &missing_files);
    append_paths(&mut message, "unexpected file", &unexpected_files);
    Err(BootstrapError::new(message))
}

fn append_paths(message: &mut String, label: &str, paths: &[&PathBuf]) {
    for path in paths {
        message.push_str(&format!("\n  {label}: {}", path.display()));
    }
}

fn compare_with_committed(repository: &Path, generated: &Path) -> Result<()> {
    let differences = differing_outputs(repository, generated)?;

    if differences.is_empty() {
        Ok(())
    } else {
        let mut message = String::from("generated Rust differs from committed source");
        for path in differences {
            message.push_str(&format!("\n  different file: {path}"));
        }
        Err(BootstrapError::new(message))
    }
}

fn differing_outputs(repository: &Path, generated: &Path) -> Result<Vec<&'static str>> {
    let mut differences = Vec::new();
    for relative in EXPECTED_OUTPUTS {
        let committed_path = repository.join(relative);
        let generated_path = generated.join(relative);
        let committed = fs::read(&committed_path)
            .map_err(|error| BootstrapError::io("cannot read", &committed_path, error))?;
        let candidate = fs::read(&generated_path)
            .map_err(|error| BootstrapError::io("cannot read", &generated_path, error))?;
        if committed != candidate {
            differences.push(*relative);
        }
    }
    Ok(differences)
}

struct StageDestination {
    path: PathBuf,
    existed: bool,
}

impl StageDestination {
    fn validate(repository: &Path, requested: &Path) -> Result<Self> {
        let current_directory = env::current_dir().map_err(|error| {
            BootstrapError::new(format!("cannot read current directory: {error}"))
        })?;
        let requested = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            current_directory.join(requested)
        };

        let metadata = fs::symlink_metadata(&requested);
        let (path, existed) = match metadata {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(BootstrapError::new(format!(
                        "stage destination may not be a symbolic link: {}",
                        requested.display()
                    )));
                }
                if !metadata.is_dir() {
                    return Err(BootstrapError::new(format!(
                        "stage destination is not a directory: {}",
                        requested.display()
                    )));
                }
                if fs::read_dir(&requested)
                    .map_err(|error| {
                        BootstrapError::io("cannot read", &requested, error)
                    })?
                    .next()
                    .is_some()
                {
                    return Err(BootstrapError::new(format!(
                        "stage destination is not empty: {}",
                        requested.display()
                    )));
                }
                (
                    requested.canonicalize().map_err(|error| {
                        BootstrapError::io("cannot resolve", &requested, error)
                    })?,
                    true,
                )
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let file_name = requested.file_name().ok_or_else(|| {
                    BootstrapError::new(format!(
                        "stage destination has no final component: {}",
                        requested.display()
                    ))
                })?;
                let parent = requested.parent().ok_or_else(|| {
                    BootstrapError::new(format!(
                        "stage destination has no parent: {}",
                        requested.display()
                    ))
                })?;
                let parent = parent.canonicalize().map_err(|error| {
                    BootstrapError::io("cannot resolve stage parent", parent, error)
                })?;
                (parent.join(file_name), false)
            }
            Err(error) => {
                return Err(BootstrapError::io(
                    "cannot inspect stage destination",
                    &requested,
                    error,
                ));
            }
        };

        let canonical = repository.canonicalize().map_err(|error| {
            BootstrapError::io("cannot resolve repository", repository, error)
        })?;
        let canonical_lit = canonical.join("lit");
        let canonical_src = canonical.join("src");
        if path == canonical
            || path.starts_with(&canonical_lit)
            || path.starts_with(&canonical_src)
            || canonical.starts_with(&path)
        {
            return Err(BootstrapError::new(format!(
                "unsafe stage destination: {}",
                path.display()
            )));
        }

        Ok(Self { path, existed })
    }

    fn install(&self, generated: PathBuf) -> Result<()> {
        if self.existed {
            let metadata = fs::symlink_metadata(&self.path).map_err(|error| {
                BootstrapError::io("cannot recheck stage destination", &self.path, error)
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(BootstrapError::new(format!(
                    "stage destination changed during generation: {}",
                    self.path.display()
                )));
            }
            if fs::read_dir(&self.path)
                .map_err(|error| BootstrapError::io("cannot recheck", &self.path, error))?
                .next()
                .is_some()
            {
                return Err(BootstrapError::new(format!(
                    "stage destination became nonempty during generation: {}",
                    self.path.display()
                )));
            }
            fs::remove_dir(&self.path).map_err(|error| {
                BootstrapError::io(
                    "cannot replace empty stage directory",
                    &self.path,
                    error,
                )
            })?;
        } else if self.path.exists() {
            return Err(BootstrapError::new(format!(
                "stage destination appeared during generation: {}",
                self.path.display()
            )));
        }

        if let Err(error) = fs::rename(&generated, &self.path) {
            if self.existed {
                let _ = fs::create_dir(&self.path);
            }
            return Err(BootstrapError::io(
                "cannot install staged tree at",
                &self.path,
                error,
            ));
        }
        Ok(())
    }
}

struct OwnedDirectory {
    path: PathBuf,
}

impl OwnedDirectory {
    fn new_in(parent: &Path, label: &str) -> Result<Self> {
        let parent = parent.canonicalize().map_err(|error| {
            BootstrapError::io("cannot resolve temporary parent", parent, error)
        })?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| BootstrapError::new(format!("system clock error: {error}")))?
            .as_nanos();

        for attempt in 0..1000_u32 {
            let path = parent.join(format!(
                ".{label}-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(BootstrapError::io(
                        "cannot create temporary directory",
                        &path,
                        error,
                    ));
                }
            }
        }

        Err(BootstrapError::new(format!(
            "cannot reserve a temporary directory below {}",
            parent.display()
        )))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "warning: cannot remove temporary directory {}: {error}",
                self.path.display()
            );
        }
    }
}
