use anyhow::{bail, Context, Result};
use clap::Parser;
use humansize::{format_size, DECIMAL};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "sizr")]
#[command(about = "A CLI tool to explore and list files and folders by size")]
#[command(version = "0.3.0")]
struct Args {
    /// Path to analyze (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: String,

    /// Number of items to display
    #[arg(short, long, default_value = "10")]
    limit: usize,

    /// Show only directories
    #[arg(short, long, conflicts_with = "files_only")]
    dirs_only: bool,

    /// Show only files
    #[arg(short, long, conflicts_with = "dirs_only")]
    files_only: bool,

    /// Display full paths instead of truncating them
    #[arg(short = 'P', long)]
    full_paths: bool,

    /// Minimum size to display (e.g., 1MB, 500KB, 2GB). Default is 0 (show all)
    #[arg(short = 'm', long, default_value = "0")]
    min_size: String,
}

#[derive(Debug, Clone)]
struct Item {
    path: String,
    size: u64,
    is_directory: bool,
}

#[derive(Debug)]
struct ScanResult {
    items: Vec<Item>,
    matching_file_size: u64,
    warnings: Vec<String>,
}

fn parse_size(size_str: &str) -> Result<u64> {
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

fn main() -> Result<()> {
    let args = Args::parse();

    // Determine what to include based on flags
    let (include_files, include_directories) = if args.dirs_only {
        (false, true)
    } else if args.files_only {
        (true, false)
    } else {
        (true, true) // Default: show both files and directories
    };

    // Parse minimum size
    let min_size_bytes = parse_size(&args.min_size)
        .context(format!("Failed to parse minimum size: {}", args.min_size))?;

    let path = Path::new(&args.path);
    if !path.exists() {
        eprintln!("Error: Path '{}' does not exist", args.path);
        std::process::exit(1);
    }

    println!("Analyzing path: {}", path.display());
    if min_size_bytes > 0 {
        println!(
            "Minimum size filter: {}",
            format_size(min_size_bytes, DECIMAL)
        );
    }
    println!("Scanning files and directories...\n");

    let start_time = Instant::now();
    let scan_result = scan_directory(
        &args.path,
        include_files,
        include_directories,
        min_size_bytes,
    )?;
    let scan_duration = start_time.elapsed();

    print_warnings(&scan_result.warnings);

    if scan_result.items.is_empty() {
        println!("No items found matching the criteria.");
        println!(
            "Total matching file size: {}",
            format_size(scan_result.matching_file_size, DECIMAL)
        );
        println!("Scan completed in {scan_duration:.2?}");
        return Ok(());
    }

    display_results(
        &scan_result.items,
        args.limit,
        scan_duration,
        args.full_paths,
        scan_result.matching_file_size,
    );

    Ok(())
}

fn scan_directory(
    path: &str,
    include_files: bool,
    include_directories: bool,
    min_size: u64,
) -> Result<ScanResult> {
    let mut items = Vec::new();
    let mut dir_sizes: HashMap<String, u64> = HashMap::new();
    let mut directories = Vec::new();
    let mut warnings = Vec::new();
    let mut matching_file_size = 0_u64;

    for entry in WalkDir::new(path) {
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
            let file_matches = size >= min_size;
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

            if include_files && file_matches {
                items.push(Item {
                    path: entry_path.to_string_lossy().to_string(),
                    size,
                    is_directory: false,
                });
            }
        }
    }

    if include_directories {
        for path_str in directories {
            let size = dir_sizes.get(&path_str).copied().unwrap_or(0);

            if size >= min_size {
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

fn print_warnings(warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }

    eprintln!(
        "Warning: skipped {} entries because they could not be read:",
        warnings.len()
    );
    for warning in warnings.iter().take(5) {
        eprintln!("  - {warning}");
    }
    if warnings.len() > 5 {
        eprintln!("  - ... and {} more", warnings.len() - 5);
    }
}

fn display_results(
    items: &[Item],
    limit: usize,
    scan_duration: std::time::Duration,
    full_paths: bool,
    matching_file_size: u64,
) {
    let display_count = std::cmp::min(items.len(), limit);

    println!("Top {display_count} largest items:");
    if full_paths {
        println!("{:<80} {:>12} Type", "Path", "Size");
        println!("{}", "-".repeat(100));
    } else {
        println!("{:<50} {:>12} Type", "Path", "Size");
        println!("{}", "-".repeat(70));
    }

    for (index, item) in items.iter().take(limit).enumerate() {
        let size_str = format_size(item.size, DECIMAL);
        let type_str = if item.is_directory { "DIR" } else { "FILE" };
        let path_display = if full_paths {
            item.path.clone()
        } else if item.path.chars().count() > 47 {
            let chars: Vec<char> = item.path.chars().collect();
            let start_idx = chars.len().saturating_sub(44);
            format!("...{}", chars[start_idx..].iter().collect::<String>())
        } else {
            item.path.clone()
        };

        if full_paths {
            println!(
                "{:2}. {:<77} {:>12} {}",
                index + 1,
                path_display,
                size_str,
                type_str
            );
        } else {
            println!(
                "{:2}. {:<47} {:>12} {}",
                index + 1,
                path_display,
                size_str,
                type_str
            );
        }
    }

    if items.len() > limit {
        println!("\n... and {} more items", items.len() - limit);
    }

    println!(
        "\nTotal matching file size: {}",
        format_size(matching_file_size, DECIMAL)
    );
    println!("Scan completed in {scan_duration:.2?}");
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
    fn files_only_and_dirs_only_conflict() {
        assert!(Args::try_parse_from(["sizr", "--files-only", "--dirs-only"]).is_err());
    }

    #[test]
    fn scan_records_walk_errors() {
        let missing_path = test_root("missing-path");

        let result = scan_directory(missing_path.to_str().unwrap(), true, true, 0).unwrap();

        assert_eq!(result.matching_file_size, 0);
        assert!(result.items.is_empty());
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn dirs_only_keeps_matching_file_size_without_summing_directories() {
        let root = create_fixture("dirs-only-total");

        let result = scan_directory(root.to_str().unwrap(), false, true, 0).unwrap();

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

        let result = scan_directory(root.to_str().unwrap(), true, true, 1_500).unwrap();

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
}
