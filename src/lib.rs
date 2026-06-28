use anyhow::{bail, Context, Result};
use humansize::{format_size, DECIMAL};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use walkdir::WalkDir;

pub const SIZE_SEMANTICS: &str = "logical_file_size_bytes";
pub const DIRECTORY_SIZE_SEMANTICS: &str = "sum_of_contained_logical_file_size_bytes";
pub const SYMLINKS_FOLLOWED: bool = false;

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub include_files: bool,
    pub include_directories: bool,
    pub min_size: u64,
}

impl ScanOptions {
    pub fn new(include_files: bool, include_directories: bool, min_size: u64) -> Self {
        Self {
            include_files,
            include_directories,
            min_size,
        }
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
    pub matching_file_size: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct JsonOutput {
    pub schema_version: u8,
    pub root_path: String,
    pub size_semantics: &'static str,
    pub directory_size_semantics: &'static str,
    pub symlinks_followed: bool,
    pub min_size_bytes: u64,
    pub limit: usize,
    pub displayed_count: usize,
    pub total_items_count: usize,
    pub total_matching_file_size_bytes: u64,
    pub total_matching_file_size_human: String,
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
    let mut items = Vec::new();
    let mut dir_sizes: HashMap<String, u64> = HashMap::new();
    let mut directories = Vec::new();
    let mut warnings = Vec::new();
    let mut matching_file_size = 0_u64;

    for entry in WalkDir::new(path.as_ref()) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(error.to_string());
                continue;
            }
        };
        let entry_path = entry.path();

        if entry.file_type().is_dir() {
            if entry.depth() > 0 {
                let path_str = entry_path.to_string_lossy().to_string();
                directories.push(path_str.clone());
                dir_sizes.entry(path_str).or_insert(0);
            }
            continue;
        }

        if entry.file_type().is_file() {
            let size = match fs::metadata(entry_path) {
                Ok(metadata) => metadata.len(),
                Err(error) => {
                    warnings.push(format!(
                        "Failed to get metadata for {}: {error}",
                        entry_path.display()
                    ));
                    continue;
                }
            };
            let file_matches = size >= options.min_size;
            if file_matches {
                matching_file_size += size;
            }

            // Add file size to parent directories inside the scanned root only.
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
        matching_file_size,
        warnings,
    })
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
        schema_version: 1,
        root_path: root_path.display().to_string(),
        size_semantics: SIZE_SEMANTICS,
        directory_size_semantics: DIRECTORY_SIZE_SEMANTICS,
        symlinks_followed: SYMLINKS_FOLLOWED,
        min_size_bytes: min_size,
        limit,
        displayed_count,
        total_items_count: scan_result.items.len(),
        total_matching_file_size_bytes: scan_result.matching_file_size,
        total_matching_file_size_human: format_human_size(scan_result.matching_file_size),
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

        assert_eq!(result.matching_file_size, 0);
        assert!(result.items.is_empty());
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn dirs_only_keeps_matching_file_size_without_summing_directories() {
        let root = create_fixture("dirs-only-total");

        let result = scan_directory(&root, ScanOptions::new(false, true, 0)).unwrap();

        assert_eq!(result.matching_file_size, 3_000);
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
    fn min_size_filter_changes_matching_file_size() {
        let root = create_fixture("min-size-total");

        let result = scan_directory(&root, ScanOptions::new(true, true, 1_500)).unwrap();

        assert_eq!(result.matching_file_size, 2_000);
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
    fn json_output_includes_limited_items_and_summary_fields() {
        let root = create_fixture("json-summary");
        let scan_result = scan_directory(&root, ScanOptions::new(true, true, 0)).unwrap();

        let output = build_json_output(&root, &scan_result, 1, Duration::from_millis(12), 0);
        let serialized = serde_json::to_string(&output).expect("JSON output should serialize");

        assert!(serialized.contains("\"schema_version\":1"));
        assert_eq!(output.schema_version, 1);
        assert_eq!(output.size_semantics, "logical_file_size_bytes");
        assert_eq!(
            output.directory_size_semantics,
            "sum_of_contained_logical_file_size_bytes"
        );
        assert!(!output.symlinks_followed);
        assert_eq!(output.limit, 1);
        assert_eq!(output.displayed_count, 1);
        assert_eq!(output.total_items_count, 4);
        assert_eq!(output.total_matching_file_size_bytes, 3_000);
        assert_eq!(output.items.len(), 1);
        assert_eq!(output.items[0].rank, 1);
        assert_eq!(output.warnings.count, 0);

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

        assert_eq!(result.matching_file_size, 0);
        assert!(result.items.is_empty());

        fs::remove_dir_all(root).expect("fixture root should be removed");
        fs::remove_dir_all(outside).expect("fixture target should be removed");
    }
}
