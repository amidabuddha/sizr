use serde_json::Value;
use sizr::{build_json_output, scan_directory, ScanOptions};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = unique_path(name);
        fs::create_dir_all(&path).expect("test directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn create_file(&self, name: &str, size: u64) {
        let file = File::create(self.path.join(name)).expect("test file should be created");
        file.set_len(size).expect("test file size should be set");
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("sizr-cli-{name}-{}-{nanos}", std::process::id()))
}

fn run_sizr(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sizr"))
        .arg("--path")
        .arg(root)
        .args(arguments)
        .output()
        .expect("sizr process should run")
}

fn parse_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout should contain valid JSON")
}

#[test]
fn library_result_exposes_native_paths_and_effective_options() {
    let root = TestDirectory::new("library-contract");
    root.create_file("data.bin", 20);
    let options = ScanOptions::new(true, false, 15);

    let result = scan_directory(root.path(), options).expect("scan should succeed");

    assert_eq!(result.root_path(), root.path());
    assert_eq!(result.options().min_size, 15);
    assert_eq!(result.items().len(), 1);
    assert_eq!(result.items()[0].path(), root.path().join("data.bin"));
    assert_eq!(result.items()[0].size(), 20);
    assert!(!result.items()[0].is_directory());
}

#[test]
fn json_mode_keeps_stdout_clean_and_applies_filters() {
    let root = TestDirectory::new("json-filter");
    root.create_file("small.bin", 10);
    root.create_file("large.bin", 20);

    let output = run_sizr(
        root.path(),
        &[
            "--files-only",
            "--min-size",
            "15B",
            "--limit",
            "1",
            "--json",
        ],
    );

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let json = parse_stdout(&output);
    assert_eq!(json["root_path"], root.path().display().to_string());
    assert_eq!(json["min_size_bytes"], 15);
    assert_eq!(json["displayed_count"], 1);
    assert_eq!(json["total_items_count"], 1);
    assert_eq!(json["total_matching_size_bytes"], 20);
    assert!(json["items"][0]["path"]
        .as_str()
        .is_some_and(|path| Path::new(path).ends_with("large.bin")));
}

#[test]
fn missing_root_exits_unsuccessfully_without_stdout() {
    let missing_root = unique_path("missing-root");

    let output = run_sizr(&missing_root, &["--json"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not exist"));
}

#[test]
fn hidden_gitignore_alias_works_end_to_end() {
    let root = TestDirectory::new("gitignore-alias");
    fs::write(root.path().join(".gitignore"), "ignored.bin\n")
        .expect("gitignore should be written");
    root.create_file("ignored.bin", 20);
    root.create_file("keep.bin", 10);

    let output = run_sizr(root.path(), &["--files-only", "--no-gitignored", "--json"]);

    assert!(output.status.success());
    let json = parse_stdout(&output);
    let paths: Vec<&str> = json["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .filter_map(|item| item["path"].as_str())
        .collect();
    assert_eq!(json["gitignore_respected"], true);
    assert!(paths
        .iter()
        .any(|path| Path::new(path).ends_with("keep.bin")));
    assert!(!paths
        .iter()
        .any(|path| Path::new(path).ends_with("ignored.bin")));
}

#[test]
fn logical_json_v4_matches_the_golden_contract() {
    let root = TestDirectory::new("json-golden");
    root.create_file("small.bin", 10);
    root.create_file("large.bin", 20);
    let scan_result =
        scan_directory(root.path(), ScanOptions::new(true, false, 0)).expect("scan should succeed");
    let output = build_json_output(&scan_result, 1, Duration::from_millis(12));
    let mut actual = serde_json::to_value(output).expect("JSON output should serialize");

    actual["root_path"] = Value::String("<ROOT>".to_owned());
    for item in actual["items"]
        .as_array_mut()
        .expect("items should be an array")
    {
        let path = item["path"].as_str().expect("item path should be a string");
        let file_name = Path::new(path)
            .file_name()
            .expect("item should be a file")
            .to_string_lossy();
        item["path"] = Value::String(format!("<ROOT>/{file_name}"));
    }

    let expected: Value = serde_json::from_str(include_str!("fixtures/logical-v4.json"))
        .expect("golden contract should contain valid JSON");
    assert_eq!(actual, expected);
}
