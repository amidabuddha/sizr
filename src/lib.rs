use anyhow::{bail, Context, Result};
use humansize::{format_size, DECIMAL};
use ignore::{DirEntry, WalkBuilder};
use serde::Serialize;
use std::collections::HashMap;
#[cfg(unix)]
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub const LOGICAL_SIZE_SEMANTICS: &str = "logical_file_size_bytes";
pub const LOGICAL_DIRECTORY_SIZE_SEMANTICS: &str = "sum_of_contained_logical_file_size_bytes";
pub const DISK_USAGE_SIZE_SEMANTICS: &str = "allocated_disk_usage_bytes";
pub const DISK_USAGE_DIRECTORY_SIZE_SEMANTICS: &str = "sum_of_contained_allocated_disk_usage_bytes";
pub const SYMLINKS_FOLLOWED: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMode {
    Logical,
    DiskUsage,
}

impl SizeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logical => "logical",
            Self::DiskUsage => "disk_usage",
        }
    }

    pub fn size_semantics(self) -> &'static str {
        match self {
            Self::Logical => LOGICAL_SIZE_SEMANTICS,
            Self::DiskUsage => DISK_USAGE_SIZE_SEMANTICS,
        }
    }

    pub fn directory_size_semantics(self) -> &'static str {
        match self {
            Self::Logical => LOGICAL_DIRECTORY_SIZE_SEMANTICS,
            Self::DiskUsage => DISK_USAGE_DIRECTORY_SIZE_SEMANTICS,
        }
    }

    pub fn total_label(self) -> &'static str {
        match self {
            Self::Logical => "Total matching file size",
            Self::DiskUsage => "Total matching disk usage",
        }
    }

    fn validate_platform(self) -> Result<()> {
        if self == Self::DiskUsage && !cfg!(unix) {
            bail!("Disk usage mode is only supported on Unix-like platforms");
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub include_files: bool,
    pub include_directories: bool,
    pub min_size: u64,
    pub size_mode: SizeMode,
    pub respect_gitignore: bool,
}

impl ScanOptions {
    pub fn new(include_files: bool, include_directories: bool, min_size: u64) -> Self {
        Self {
            include_files,
            include_directories,
            min_size,
            size_mode: SizeMode::Logical,
            respect_gitignore: false,
        }
    }

    pub fn with_size_mode(mut self, size_mode: SizeMode) -> Self {
        self.size_mode = size_mode;
        self
    }

    pub fn with_respect_gitignore(mut self, respect_gitignore: bool) -> Self {
        self.respect_gitignore = respect_gitignore;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
}

#[derive(Debug)]
pub struct ScanResult {
    pub items: Vec<Item>,
    pub matching_total_size: u64,
    pub size_mode: SizeMode,
    pub respect_gitignore: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct JsonOutput {
    pub schema_version: u8,
    pub root_path: String,
    pub size_mode: &'static str,
    pub size_semantics: &'static str,
    pub directory_size_semantics: &'static str,
    pub symlinks_followed: bool,
    pub gitignore_respected: bool,
    pub min_size_bytes: u64,
    pub limit: usize,
    pub displayed_count: usize,
    pub total_items_count: usize,
    pub total_matching_size_bytes: u64,
    pub total_matching_size_human: String,
    pub elapsed_ms: f64,
    pub items: Vec<JsonItem>,
    pub warnings: JsonWarnings,
}

#[derive(Debug, Serialize)]
pub struct JsonItem {
    pub rank: usize,
    pub path: String,
    #[serde(rename = "type")]
    pub item_type: &'static str,
    pub size_bytes: u64,
    pub size_human: String,
}

#[derive(Debug, Serialize)]
pub struct JsonWarnings {
    pub count: usize,
    pub messages: Vec<String>,
}

pub fn parse_size(size_str: &str) -> Result<u64> {
    let size_str = size_str.trim();
    if size_str.is_empty() {
        bail!("Size cannot be empty");
    }

    let size_str = size_str.to_uppercase();
    let (number_part, unit_part) = if let Some(pos) = size_str.find(|c: char| c.is_alphabetic()) {
        (&size_str[..pos], &size_str[pos..])
    } else {
        (size_str.as_str(), "")
    };

    let number_part = number_part.trim();
    let number: f64 = number_part
        .parse()
        .with_context(|| format!("Invalid number in size: {number_part}"))?;
    if !number.is_finite() || number < 0.0 {
        bail!("Size must be a finite, non-negative number");
    }

    let multiplier = match unit_part {
        "" | "B" => 1,
        "KB" => 1_024,
        "MB" => 1_024 * 1_024,
        "GB" => 1_024 * 1_024 * 1_024,
        "TB" => 1_024_u64.pow(4),
        _ => bail!("Unknown size unit: {unit_part}. Use B, KB, MB, GB, or TB"),
    };

    let bytes = number * multiplier as f64;
    if bytes > u64::MAX as f64 {
        bail!("Size is too large");
    }

    Ok(bytes as u64)
}

pub fn format_human_size(size: u64) -> String {
    format_size(size, DECIMAL)
}

pub fn scan_directory(path: impl AsRef<Path>, options: ScanOptions) -> Result<ScanResult> {
    options.size_mode.validate_platform()?;

    let mut items = Vec::new();
    let mut dir_sizes: HashMap<String, u64> = HashMap::new();
    let mut directories = Vec::new();
    let mut warnings = Vec::new();
    let mut matching_total_size = 0_u64;
    let mut hardlinks = HardlinkTracker::default();

    for entry in walk_entries(path.as_ref(), options.respect_gitignore) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(error.to_string());
                continue;
            }
        };
        let entry_path = entry.path();
        let Some(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            if entry.depth() > 0 {
                let path_str = entry_path.to_string_lossy().to_string();
                directories.push(path_str.clone());
                dir_sizes.entry(path_str).or_insert(0);
            }
            continue;
        }

        if file_type.is_file() {
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    warnings.push(format!(
                        "Failed to get metadata for {}: {error}",
                        entry_path.display()
                    ));
                    continue;
                }
            };
            let size = match measure_file(entry_path, &metadata, options.size_mode, &mut hardlinks)
            {
                Ok(size) => size,
                Err(error) => {
                    warnings.push(error);
                    continue;
                }
            };
            let file_matches = size >= options.min_size;
            if file_matches {
                matching_total_size += size;
            }

            // Add the selected size metric to parent directories inside the scanned root only.
            let mut current_path = entry_path.parent();
            for _ in 0..entry.depth() {
                let Some(parent) = current_path else {
                    break;
                };
                let parent_str = parent.to_string_lossy().to_string();
                *dir_sizes.entry(parent_str).or_insert(0) += size;
                current_path = parent.parent();
            }

            if options.include_files && file_matches {
                items.push(Item {
                    path: entry_path.to_string_lossy().to_string(),
                    size,
                    is_directory: false,
                });
            }
        }
    }

    if options.include_directories {
        for path_str in directories {
            let size = dir_sizes.get(&path_str).copied().unwrap_or(0);

            if size >= options.min_size {
                items.push(Item {
                    path: path_str,
                    size,
                    is_directory: true,
                });
            }
        }
    }

    items.sort_by(|a, b| b.size.cmp(&a.size));

    Ok(ScanResult {
        items,
        matching_total_size,
        size_mode: options.size_mode,
        respect_gitignore: options.respect_gitignore,
        warnings,
    })
}

fn walk_entries(path: &Path, respect_gitignore: bool) -> ignore::Walk {
    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(respect_gitignore)
        .git_global(respect_gitignore)
        .git_exclude(respect_gitignore)
        .require_git(false)
        .parents(respect_gitignore)
        .follow_links(false);

    if respect_gitignore {
        builder.filter_entry(|entry| entry.depth() == 0 || !is_git_metadata_dir(entry));
    }

    builder.build()
}

fn is_git_metadata_dir(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
        && entry.file_name() == OsStr::new(".git")
}

#[derive(Default)]
struct HardlinkTracker {
    #[cfg(unix)]
    seen: HashSet<(u64, u64)>,
}

fn measure_file(
    path: &Path,
    metadata: &fs::Metadata,
    size_mode: SizeMode,
    hardlinks: &mut HardlinkTracker,
) -> std::result::Result<u64, String> {
    match size_mode {
        SizeMode::Logical => Ok(metadata.len()),
        SizeMode::DiskUsage => disk_usage_bytes(metadata, hardlinks)
            .map_err(|error| format!("Failed to get disk usage for {}: {error}", path.display())),
    }
}

#[cfg(unix)]
fn disk_usage_bytes(metadata: &fs::Metadata, hardlinks: &mut HardlinkTracker) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;

    if metadata.nlink() > 1 && !hardlinks.seen.insert((metadata.dev(), metadata.ino())) {
        return Ok(0);
    }

    Ok(metadata.blocks().saturating_mul(512))
}

#[cfg(not(unix))]
fn disk_usage_bytes(_metadata: &fs::Metadata, _hardlinks: &mut HardlinkTracker) -> Result<u64> {
    bail!("disk usage is only supported on Unix-like platforms")
}

pub fn build_json_output(
    root_path: &Path,
    scan_result: &ScanResult,
    limit: usize,
    scan_duration: Duration,
    min_size: u64,
) -> JsonOutput {
    let displayed_count = std::cmp::min(scan_result.items.len(), limit);
    let items = scan_result
        .items
        .iter()
        .take(limit)
        .enumerate()
        .map(|(index, item)| JsonItem {
            rank: index + 1,
            path: item.path.clone(),
            item_type: if item.is_directory { "dir" } else { "file" },
            size_bytes: item.size,
            size_human: format_human_size(item.size),
        })
        .collect();

    JsonOutput {
        schema_version: 3,
        root_path: root_path.display().to_string(),
        size_mode: scan_result.size_mode.as_str(),
        size_semantics: scan_result.size_mode.size_semantics(),
        directory_size_semantics: scan_result.size_mode.directory_size_semantics(),
        symlinks_followed: SYMLINKS_FOLLOWED,
        gitignore_respected: scan_result.respect_gitignore,
        min_size_bytes: min_size,
        limit,
        displayed_count,
        total_items_count: scan_result.items.len(),
        total_matching_size_bytes: scan_result.matching_total_size,
        total_matching_size_human: format_human_size(scan_result.matching_total_size),
        elapsed_ms: scan_duration.as_secs_f64() * 1000.0,
        items,
        warnings: JsonWarnings {
            count: scan_result.warnings.len(),
            messages: scan_result.warnings.clone(),
        },
    }
}

pub fn write_json_output<W: Write>(mut writer: W, output: &JsonOutput) -> Result<()> {
    serde_json::to_writer_pretty(&mut writer, output).context("Failed to serialize JSON output")?;
    writeln!(&mut writer).context("Failed to write JSON output")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("sizr-{name}-{}-{nanos}", std::process::id()))
    }

    fn create_file(path: &Path, size: u64) {
        let file = File::create(path).expect("test file should be created");
        file.set_len(size).expect("test file size should be set");
    }

    #[cfg(unix)]
    fn create_written_file(path: &Path, size: usize) {
        use std::io::Write;

        let mut file = File::create(path).expect("test file should be created");
        file.write_all(&vec![0; size])
            .expect("test file bytes should be written");
    }

    #[cfg(unix)]
    fn allocated_size(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;

        fs::metadata(path)
            .expect("test file metadata should be read")
            .blocks()
            .saturating_mul(512)
    }

    fn create_fixture(name: &str) -> PathBuf {
        let root = test_root(name);
        fs::create_dir_all(root.join("a")).expect("fixture directory a should be created");
        fs::create_dir_all(root.join("b")).expect("fixture directory b should be created");
        create_file(&root.join("a").join("small.bin"), 1_000);
        create_file(&root.join("b").join("large.bin"), 2_000);
        root
    }

    fn file_name_is(path: &str, expected: &str) -> bool {
        Path::new(path).file_name() == Some(std::ffi::OsStr::new(expected))
    }

    fn parent_name_is(path: &str, expected: &str) -> bool {
        Path::new(path).parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new(expected))
    }

    #[test]
    fn parse_size_accepts_supported_units() {
        assert_eq!(parse_size("500").unwrap(), 500);
        assert_eq!(parse_size("500B").unwrap(), 500);
        assert_eq!(parse_size("1KB").unwrap(), 1_024);
        assert_eq!(parse_size("1.5MB").unwrap(), 1_572_864);
        assert_eq!(parse_size("2GB").unwrap(), 2_147_483_648);
        assert_eq!(parse_size("1TB").unwrap(), 1_099_511_627_776);
    }

    #[test]
    fn parse_size_rejects_invalid_values() {
        assert!(parse_size("").is_err());
        assert!(parse_size("-1MB").is_err());
        assert!(parse_size("NaNMB").is_err());
        assert!(parse_size("1XB").is_err());
    }

    #[test]
    fn scan_records_walk_errors() {
        let missing_path = test_root("missing-path");

        let result = scan_directory(missing_path, ScanOptions::new(true, true, 0)).unwrap();

        assert_eq!(result.matching_total_size, 0);
        assert!(result.items.is_empty());
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn dirs_only_keeps_matching_total_without_summing_directories() {
        let root = create_fixture("dirs-only-total");

        let result = scan_directory(&root, ScanOptions::new(false, true, 0)).unwrap();

        assert_eq!(result.matching_total_size, 3_000);
        assert_eq!(result.items.len(), 2);
        assert!(result.items.iter().all(|item| item.is_directory));
        assert_eq!(
            result
                .items
                .iter()
                .find(|item| file_name_is(&item.path, "b"))
                .unwrap()
                .size,
            2_000
        );

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn min_size_filter_changes_matching_total() {
        let root = create_fixture("min-size-total");

        let result = scan_directory(&root, ScanOptions::new(true, true, 1_500)).unwrap();

        assert_eq!(result.matching_total_size, 2_000);
        assert_eq!(result.items.len(), 2);
        assert!(result.items.iter().any(|item| {
            !item.is_directory
                && file_name_is(&item.path, "large.bin")
                && parent_name_is(&item.path, "b")
        }));
        assert!(result
            .items
            .iter()
            .any(|item| item.is_directory && file_name_is(&item.path, "b")));

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn unicode_paths_are_scanned_without_loss() {
        let root = test_root("unicode-paths");
        let nested = root.join("данни-サイズ");
        fs::create_dir_all(&nested).expect("unicode fixture directory should be created");
        create_file(&nested.join("файл.bin"), 1_234);

        let result = scan_directory(&root, ScanOptions::new(true, true, 0)).unwrap();

        assert!(result
            .items
            .iter()
            .any(|item| item.path.contains("данни-サイズ")));
        assert!(result
            .items
            .iter()
            .any(|item| item.path.contains("файл.bin")));

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn json_output_includes_limited_items_and_summary_fields() {
        let root = create_fixture("json-summary");
        let scan_result = scan_directory(&root, ScanOptions::new(true, true, 0)).unwrap();

        let output = build_json_output(&root, &scan_result, 1, Duration::from_millis(12), 0);
        let serialized = serde_json::to_string(&output).expect("JSON output should serialize");

        assert!(serialized.contains("\"schema_version\":3"));
        assert_eq!(output.schema_version, 3);
        assert_eq!(output.size_mode, "logical");
        assert_eq!(output.size_semantics, "logical_file_size_bytes");
        assert_eq!(
            output.directory_size_semantics,
            "sum_of_contained_logical_file_size_bytes"
        );
        assert!(!output.symlinks_followed);
        assert!(!output.gitignore_respected);
        assert_eq!(output.limit, 1);
        assert_eq!(output.displayed_count, 1);
        assert_eq!(output.total_items_count, 4);
        assert_eq!(output.total_matching_size_bytes, 3_000);
        assert_eq!(output.items.len(), 1);
        assert_eq!(output.items[0].rank, 1);
        assert_eq!(output.warnings.count, 0);

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn disk_usage_mode_uses_allocated_blocks() {
        let root = test_root("disk-usage");
        fs::create_dir_all(&root).expect("fixture root should be created");
        let file_path = root.join("allocated.bin");
        create_written_file(&file_path, 4_096);
        let expected_size = allocated_size(&file_path);

        let result = scan_directory(
            &root,
            ScanOptions::new(true, false, 0).with_size_mode(SizeMode::DiskUsage),
        )
        .unwrap();

        assert_eq!(result.size_mode, SizeMode::DiskUsage);
        assert_eq!(result.matching_total_size, expected_size);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].size, expected_size);

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn disk_usage_mode_counts_hardlinked_inode_once() {
        let root = test_root("disk-usage-hardlinks");
        fs::create_dir_all(&root).expect("fixture root should be created");
        let original = root.join("original.bin");
        let linked = root.join("linked.bin");
        create_written_file(&original, 4_096);
        fs::hard_link(&original, &linked).expect("hardlink should be created");
        let expected_size = allocated_size(&original);

        let result = scan_directory(
            &root,
            ScanOptions::new(true, false, 0).with_size_mode(SizeMode::DiskUsage),
        )
        .unwrap();

        assert_eq!(result.matching_total_size, expected_size);
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].size, expected_size);
        assert_eq!(result.items[1].size, 0);

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn json_output_reports_disk_usage_mode() {
        let root = test_root("json-disk-usage");
        fs::create_dir_all(&root).expect("fixture root should be created");
        create_written_file(&root.join("allocated.bin"), 4_096);
        let scan_result = scan_directory(
            &root,
            ScanOptions::new(true, true, 0).with_size_mode(SizeMode::DiskUsage),
        )
        .unwrap();

        let output = build_json_output(&root, &scan_result, 1, Duration::from_millis(12), 0);

        assert_eq!(output.size_mode, "disk_usage");
        assert_eq!(output.size_semantics, "allocated_disk_usage_bytes");
        assert_eq!(
            output.directory_size_semantics,
            "sum_of_contained_allocated_disk_usage_bytes"
        );

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn write_json_output_serializes_with_trailing_newline() {
        let root = create_fixture("json-writer");
        let scan_result = scan_directory(&root, ScanOptions::new(true, true, 0)).unwrap();
        let output = build_json_output(&root, &scan_result, 1, Duration::from_millis(12), 0);
        let mut buffer = Vec::new();

        write_json_output(&mut buffer, &output).expect("JSON output should be written");

        assert!(buffer.ends_with(b"\n"));
        serde_json::from_slice::<serde_json::Value>(&buffer).expect("output should be valid JSON");

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn gitignored_files_are_included_by_default() {
        let root = test_root("gitignored-default");
        fs::create_dir_all(&root).expect("fixture root should be created");
        fs::write(root.join(".gitignore"), "ignored.bin\n").expect("gitignore should be written");
        create_file(&root.join("ignored.bin"), 1_000);
        create_file(&root.join("keep.bin"), 2_000);

        let result = scan_directory(&root, ScanOptions::new(true, false, 0)).unwrap();

        assert!(!result.respect_gitignore);
        assert!(result
            .items
            .iter()
            .any(|item| file_name_is(&item.path, "ignored.bin")));
        assert!(result
            .items
            .iter()
            .any(|item| file_name_is(&item.path, "keep.bin")));

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn respect_gitignore_skips_ignored_files() {
        let root = test_root("respect-gitignore");
        fs::create_dir_all(&root).expect("fixture root should be created");
        fs::write(root.join(".gitignore"), "ignored.bin\n").expect("gitignore should be written");
        create_file(&root.join("ignored.bin"), 1_000);
        create_file(&root.join("keep.bin"), 2_000);

        let result = scan_directory(
            &root,
            ScanOptions::new(true, false, 0).with_respect_gitignore(true),
        )
        .unwrap();
        let output = build_json_output(&root, &result, 10, Duration::from_millis(12), 0);

        assert!(result.respect_gitignore);
        assert!(output.gitignore_respected);
        assert!(!result
            .items
            .iter()
            .any(|item| file_name_is(&item.path, "ignored.bin")));
        assert!(result
            .items
            .iter()
            .any(|item| file_name_is(&item.path, "keep.bin")));

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn respect_gitignore_skips_git_metadata_directory() {
        let root = test_root("respect-gitignore-gitdir");
        fs::create_dir_all(root.join(".git").join("objects"))
            .expect("git metadata directory should be created");
        create_file(&root.join(".git").join("objects").join("object"), 1_000);
        create_file(&root.join("keep.bin"), 2_000);

        let result = scan_directory(
            &root,
            ScanOptions::new(true, true, 0).with_respect_gitignore(true),
        )
        .unwrap();

        assert!(!result.items.iter().any(|item| item.path.contains(".git")));
        assert!(result
            .items
            .iter()
            .any(|item| file_name_is(&item.path, "keep.bin")));

        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn scan_does_not_follow_symlinks_by_default() {
        let root = test_root("symlink-root");
        let outside = test_root("symlink-target");
        fs::create_dir_all(&root).expect("fixture root should be created");
        fs::create_dir_all(&outside).expect("fixture target should be created");
        create_file(&outside.join("outside.bin"), 4_096);
        std::os::unix::fs::symlink(outside.join("outside.bin"), root.join("linked.bin"))
            .expect("symlink should be created");

        let result = scan_directory(&root, ScanOptions::new(true, true, 0)).unwrap();

        assert_eq!(result.matching_total_size, 0);
        assert!(result.items.is_empty());

        fs::remove_dir_all(root).expect("fixture root should be removed");
        fs::remove_dir_all(outside).expect("fixture target should be removed");
    }
}
