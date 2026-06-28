use anyhow::{Context, Result};
use clap::Parser;
use sizr::{
    build_json_output, format_human_size, parse_size, scan_directory, write_json_output, Item,
    ScanOptions,
};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "sizr")]
#[command(about = "Explore and rank files and folders by logical file size")]
#[command(version)]
struct Args {
    /// Path to scan for logical file sizes (defaults to current directory)
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

    /// Minimum logical size to display (e.g., 1MB, 500KB, 2GB). Default is 0 (show all)
    #[arg(short = 'm', long, default_value = "0")]
    min_size: String,

    /// Output machine-readable JSON instead of the human table
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let min_size_bytes = parse_size(&args.min_size)
        .context(format!("Failed to parse minimum size: {}", args.min_size))?;
    let scan_options = scan_options_from_args(&args, min_size_bytes);

    let path = Path::new(&args.path);
    if !path.exists() {
        eprintln!("Error: Path '{}' does not exist", args.path);
        std::process::exit(1);
    }

    if !args.json {
        println!("Analyzing path: {}", path.display());
        if min_size_bytes > 0 {
            println!("Minimum size filter: {}", format_human_size(min_size_bytes));
        }
        println!("Scanning files and directories...\n");
    }

    let start_time = Instant::now();
    let scan_result = scan_directory(path, scan_options)?;
    let scan_duration = start_time.elapsed();

    if args.json {
        let output = build_json_output(
            path,
            &scan_result,
            args.limit,
            scan_duration,
            min_size_bytes,
        );
        write_json_output(io::stdout().lock(), &output)?;
        return Ok(());
    }

    print_warnings(&scan_result.warnings);

    if scan_result.items.is_empty() {
        println!("No items found matching the criteria.");
        println!(
            "Total matching file size: {}",
            format_human_size(scan_result.matching_file_size)
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

fn scan_options_from_args(args: &Args, min_size: u64) -> ScanOptions {
    let (include_files, include_directories) = if args.dirs_only {
        (false, true)
    } else if args.files_only {
        (true, false)
    } else {
        (true, true)
    };

    ScanOptions::new(include_files, include_directories, min_size)
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
    scan_duration: Duration,
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
        let size_str = format_human_size(item.size);
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
        format_human_size(matching_file_size)
    );
    println!("Scan completed in {scan_duration:.2?}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_only_and_dirs_only_conflict() {
        assert!(Args::try_parse_from(["sizr", "--files-only", "--dirs-only"]).is_err());
    }
}
