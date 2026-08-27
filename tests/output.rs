use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use litweb::output::{
    InvalidOutputPathReason, OutputErrorKind, PlannedOutput, write_planned_outputs,
    write_planned_outputs_protecting,
};
use litweb::parser::SourceOrigin;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "litweb-output-{label}-{}-{number}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn planned(path: impl Into<PathBuf>, bytes: &str, line: usize) -> PlannedOutput {
    PlannedOutput {
        relative_path: path.into(),
        bytes: bytes.as_bytes().to_vec(),
        origin: SourceOrigin::new("input.lit", line),
    }
}

#[test]
fn unconfined_paths_are_rejected_before_the_output_directory_is_created() {
    let directory = TestDirectory::new("invalid-relative-paths");
    let output_directory = directory.path().join("generated");
    let absolute_escape = directory.path().join("absolute-escape.txt");
    let parent_escape = directory.path().join("parent-escape.txt");
    let cases = [
        (PathBuf::new(), InvalidOutputPathReason::Empty),
        (PathBuf::from("."), InvalidOutputPathReason::Empty),
        (absolute_escape.clone(), InvalidOutputPathReason::Absolute),
        (
            PathBuf::from("../parent-escape.txt"),
            InvalidOutputPathReason::ParentTraversal,
        ),
    ];

    for (line, (path, reason)) in cases.into_iter().enumerate() {
        let outputs = [planned(path.clone(), "escaped", line + 1)];
        let error = write_planned_outputs_protecting(&outputs, &output_directory, &[])
            .unwrap_err();
        assert_eq!(error.kind, OutputErrorKind::InvalidRelativePath { reason });
        assert_eq!(error.path, path);
        assert!(!output_directory.exists());
        assert!(!absolute_escape.exists());
        assert!(!parent_escape.exists());
    }
}

#[cfg(windows)]
#[test]
fn platform_prefixed_paths_are_rejected_before_writing() {
    let directory = TestDirectory::new("prefixed-relative-path");
    let output_directory = directory.path().join("generated");
    let path = PathBuf::from(r"C:outside.txt");
    let outputs = [planned(path.clone(), "escaped", 2)];

    let error =
        write_planned_outputs_protecting(&outputs, &output_directory, &[]).unwrap_err();
    assert_eq!(
        error.kind,
        OutputErrorKind::InvalidRelativePath {
            reason: InvalidOutputPathReason::PlatformPrefix,
        }
    );
    assert_eq!(error.path, path);
    assert!(!output_directory.exists());
}

#[test]
fn every_destination_is_preflighted_before_any_file_is_replaced() {
    let directory = TestDirectory::new("preflight");
    let output_directory = directory.path().join("output");
    fs::create_dir_all(&output_directory).unwrap();
    fs::write(output_directory.join("nested"), "not a directory").unwrap();
    let outputs = [
        planned("alpha.txt", "new alpha", 2),
        planned("nested/beta.txt", "new beta", 5),
    ];

    let error =
        write_planned_outputs_protecting(&outputs, &output_directory, &[]).unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::ParentIsNotDirectory);
    assert!(!output_directory.join("alpha.txt").exists());
    assert_eq!(
        fs::read_to_string(output_directory.join("nested")).unwrap(),
        "not a directory"
    );
}

#[test]
fn an_existing_directory_cannot_be_replaced_by_an_output_file() {
    let directory = TestDirectory::new("destination-directory");
    let destination = directory.path().join("output.txt");
    fs::create_dir(&destination).unwrap();
    let outputs = [planned("output.txt", "new output", 2)];

    let error = write_planned_outputs(
        &outputs,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::DestinationIsDirectory);
    assert!(destination.is_dir());
}

#[test]
fn a_protected_input_cannot_be_replaced() {
    let directory = TestDirectory::new("input-collision");
    let input = directory.path().join("output.txt");
    fs::write(&input, "original input").unwrap();
    let outputs = [planned("output.txt", "new output", 2)];

    let error = write_planned_outputs(&outputs, directory.path(), &input).unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::InputCollision);
    assert_eq!(fs::read_to_string(&input).unwrap(), "original input");
}

#[test]
fn duplicate_destinations_are_rejected_before_writing() {
    let directory = TestDirectory::new("duplicate");
    let outputs = [
        planned("same.txt", "first", 2),
        planned("same.txt", "second", 5),
    ];

    let error = write_planned_outputs(
        &outputs,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::DuplicateDestination);
    assert_eq!(error.origin.line, 5);
    assert!(!directory.path().join("same.txt").exists());
}

#[test]
fn a_current_directory_component_participates_in_duplicate_detection() {
    let directory = TestDirectory::new("current-directory-duplicate");
    let outputs = [
        planned("same.txt", "first", 2),
        planned("./same.txt", "second", 5),
    ];

    let error = write_planned_outputs(
        &outputs,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::DuplicateDestination);
    assert_eq!(error.origin.line, 5);
    assert!(!directory.path().join("same.txt").exists());
}

#[test]
fn a_planned_file_cannot_be_another_outputs_parent_directory() {
    let directory = TestDirectory::new("planned-parent");
    let outputs = [
        planned("assets", "parent file", 2),
        planned("assets/math.js", "child file", 5),
    ];

    let error = write_planned_outputs(
        &outputs,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap_err();
    assert_eq!(error.kind, OutputErrorKind::ParentIsPlannedFile);
    assert_eq!(error.origin.line, 5);
    assert!(!directory.path().join("assets").exists());
}

#[test]
fn the_documented_greeting_is_installed_without_temporary_residue() {
    let directory = TestDirectory::new("documented-greeting");
    let output_directory = directory.path().join("generated");
    fs::create_dir(&output_directory).unwrap();
    let destination = output_directory.join("hello.txt");
    fs::write(&destination, "old contents").unwrap();
    let outputs = [PlannedOutput {
        relative_path: PathBuf::from("hello.txt"),
        bytes: b"Good morning\nfrom Litweb\n".to_vec(),
        origin: SourceOrigin::new("greeting.lit", 5),
    }];

    write_planned_outputs(
        &outputs,
        &output_directory,
        &directory.path().join("greeting.lit"),
    )
    .unwrap();
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"Good morning\nfrom Litweb\n"
    );
    assert_eq!(
        fs::read_dir(&output_directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec!["hello.txt"]
    );
}

#[cfg(unix)]
#[test]
fn replacing_an_existing_file_preserves_its_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDirectory::new("preserve-permissions");
    let destination = directory.path().join("output.txt");
    fs::write(&destination, "old contents").unwrap();
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o741)).unwrap();
    let outputs = [planned("output.txt", "new contents", 2)];

    write_planned_outputs(
        &outputs,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap();

    assert_eq!(fs::read(&destination).unwrap(), b"new contents");
    assert_eq!(
        fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
        0o741
    );
}

#[test]
fn a_new_file_is_created_normally() {
    let directory = TestDirectory::new("new-file");
    let output_directory = directory.path().join("generated");
    let outputs = [planned("nested/output.txt", "new contents", 2)];

    write_planned_outputs(
        &outputs,
        &output_directory,
        &directory.path().join("input.lit"),
    )
    .unwrap();

    assert_eq!(
        fs::read(output_directory.join("nested/output.txt")).unwrap(),
        b"new contents"
    );
}

#[cfg(windows)]
#[test]
fn a_read_only_destination_is_not_cleared_or_replaced() {
    let directory = TestDirectory::new("readonly-destination");
    let destination = directory.path().join("output.txt");
    fs::write(&destination, "old contents").unwrap();
    let mut permissions = fs::metadata(&destination).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&destination, permissions).unwrap();
    let outputs = [planned("output.txt", "new contents", 2)];

    let error = write_planned_outputs(
        &outputs,
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap_err();
    assert_eq!(
        error.kind,
        OutputErrorKind::Io {
            action: litweb::output::OutputAction::ReplaceDestination,
            error: std::io::ErrorKind::PermissionDenied,
        }
    );
    assert_eq!(fs::read_to_string(&destination).unwrap(), "old contents");
    assert_eq!(
        fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec!["output.txt"]
    );

    let mut permissions = fs::metadata(&destination).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(destination, permissions).unwrap();
}

#[test]
fn tangler_keeps_the_previous_output_api_paths() {
    let directory = TestDirectory::new("compatibility-reexports");
    let output: litweb::tangler::PlannedOutput = planned("output.txt", "contents", 2);
    let kind: litweb::tangler::OutputErrorKind = OutputErrorKind::InputCollision;

    assert_eq!(kind, OutputErrorKind::InputCollision);
    litweb::tangler::write_planned_outputs(
        &[output],
        directory.path(),
        &directory.path().join("input.lit"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("output.txt")).unwrap(),
        "contents"
    );
}
