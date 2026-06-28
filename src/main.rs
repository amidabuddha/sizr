use anyhow::{Context, Result};
use clap::Parser;
use sizr::{
    build_json_output, format_human_size, parse_size, scan_directory, write_json_output,
    ScanOptions, SizeMode,
};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "sizr")]
#[command(about = "Explore and rank files and folders by size")]
#[command(version)]
struct Args {
    /// Path to scan for sizes (defaults to current directory)
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

    /// Minimum selected size metric to display (e.g., 1MB, 500KB, 2GB). Default is 0 (show all)
    #[arg(short = 'm', long, default_value = "0")]
    min_size: String,

    /// Rank by allocated disk usage instead of logical file size
    #[arg(long, visible_alias = "du", conflicts_with = "both")]
    disk_usage: bool,

    /// Show logical size and disk usage side by side, ranked by logical size
    #[arg(long, visible_aliases = ["combined", "compare"])]
    both: bool,

    /// Skip files ignored by .gitignore, .git/info/exclude, or global gitignore rules
    #[arg(long, visible_alias = "no-gitignored")]
    respect_gitignore: bool,

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
        if args.both {
            println!("Size metric: logical size + disk usage (ranked by logical size)");
        } else if args.disk_usage {
            println!("Size metric: disk usage (allocated filesystem blocks)");
        }
        if args.respect_gitignore {
            println!("Gitignore: respecting git ignore rules");
        }
        if min_size_bytes > 0 {
            println!(
                "Minimum {} filter: {}",
                if args.both {
                    "logical size"
                } else if args.disk_usage {
                    "disk usage"
                } else {
                    "size"
                },
                format_human_size(min_size_bytes)
            );
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
        print_totals(&scan_result);
        println!("Scan completed in {scan_duration:.2?}");
        return Ok(());
    }

    display_results(&scan_result, args.limit, scan_duration, args.full_paths);

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

    let size_mode = if args.both {
        SizeMode::Combined
    } else if args.disk_usage {
        SizeMode::DiskUsage
    } else {
        SizeMode::Logical
    };

    ScanOptions::new(include_files, include_directories, min_size)
        .with_size_mode(size_mode)
        .with_respect_gitignore(args.respect_gitignore)
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

#[derive(Debug, Clone, Copy)]
struct TableLayout {
    path_width: usize,
    metric_columns: MetricColumns,
    full_paths: bool,
}

#[derive(Debug, Clone, Copy)]
enum MetricColumns {
    Single,
    Combined,
}

const RANK_PREFIX_WIDTH: usize = 4;
const SIZE_COLUMN_WIDTH: usize = 12;
const TYPE_COLUMN_WIDTH: usize = 4;
const TRUNCATION_MARKER: &str = "...";

impl TableLayout {
    fn new(size_mode: SizeMode, full_paths: bool) -> Self {
        let metric_columns = if size_mode.is_combined() {
            MetricColumns::Combined
        } else {
            MetricColumns::Single
        };

        let path_width = match (metric_columns, full_paths) {
            (MetricColumns::Combined, true) => 69,
            (MetricColumns::Combined, false) => 39,
            (MetricColumns::Single, true) => 77,
            (MetricColumns::Single, false) => 47,
        };

        Self {
            path_width,
            metric_columns,
            full_paths,
        }
    }

    fn metric_count(self) -> usize {
        match self.metric_columns {
            MetricColumns::Single => 1,
            MetricColumns::Combined => 2,
        }
    }

    fn header_path_width(self) -> usize {
        self.path_width + RANK_PREFIX_WIDTH - 1
    }

    fn separator_width(self) -> usize {
        RANK_PREFIX_WIDTH
            + self.path_width
            + self.metric_count() * (SIZE_COLUMN_WIDTH + 1)
            + TYPE_COLUMN_WIDTH
            + 1
    }

    fn format_path(self, path: &str) -> String {
        if self.full_paths {
            return path.to_owned();
        }

        let char_count = path.chars().count();
        if char_count <= self.path_width {
            return path.to_owned();
        }

        let tail_chars = self.path_width.saturating_sub(TRUNCATION_MARKER.len());
        let chars: Vec<char> = path.chars().collect();
        let start_idx = chars.len().saturating_sub(tail_chars);
        format!(
            "{TRUNCATION_MARKER}{}",
            chars[start_idx..].iter().collect::<String>()
        )
    }
}

fn display_results(
    scan_result: &sizr::ScanResult,
    limit: usize,
    scan_duration: Duration,
    full_paths: bool,
) {
    let items = &scan_result.items;
    let size_mode = scan_result.size_mode;
    let display_count = std::cmp::min(items.len(), limit);
    let layout = TableLayout::new(size_mode, full_paths);

    println!("Top {display_count} largest items:");
    print_table_header(layout);

    for (index, item) in items.iter().take(limit).enumerate() {
        let type_str = if item.is_directory { "DIR" } else { "FILE" };
        let path_display = layout.format_path(&item.path);

        if size_mode.is_combined() {
            let logical_size = format_human_size(item.logical_size);
            let disk_usage = format_human_size(item.disk_usage_size.unwrap_or(0));

            print_combined_row(
                layout,
                index + 1,
                &path_display,
                &logical_size,
                &disk_usage,
                type_str,
            );
        } else {
            let size_str = format_human_size(item.size);

            print_single_row(layout, index + 1, &path_display, &size_str, type_str);
        }
    }

    if items.len() > limit {
        println!("\n... and {} more items", items.len() - limit);
    }

    print_totals_for_mode(
        scan_result.matching_total_size,
        scan_result.matching_logical_size,
        scan_result.matching_disk_usage_size,
        size_mode,
        true,
    );
    println!("Scan completed in {scan_duration:.2?}");
}

fn print_table_header(layout: TableLayout) {
    if layout.full_paths {
        print_full_path_table_header(layout);
        return;
    }

    match layout.metric_columns {
        MetricColumns::Combined => {
            println!(
                "{:<path_width$} {:>metric_width$} {:>metric_width$} Type",
                "Path",
                "Logical",
                "Disk Usage",
                path_width = layout.header_path_width(),
                metric_width = SIZE_COLUMN_WIDTH
            );
        }
        MetricColumns::Single => {
            println!(
                "{:<path_width$} {:>metric_width$} Type",
                "Path",
                "Size",
                path_width = layout.header_path_width(),
                metric_width = SIZE_COLUMN_WIDTH
            );
        }
    }
    println!("{}", "-".repeat(layout.separator_width()));
}

fn print_full_path_table_header(layout: TableLayout) {
    match layout.metric_columns {
        MetricColumns::Combined => {
            println!(
                "{:<rank_width$}{:>metric_width$} {:>metric_width$} {:<type_width$} Path",
                "",
                "Logical",
                "Disk Usage",
                "Type",
                rank_width = RANK_PREFIX_WIDTH,
                metric_width = SIZE_COLUMN_WIDTH,
                type_width = TYPE_COLUMN_WIDTH
            );
        }
        MetricColumns::Single => {
            println!(
                "{:<rank_width$}{:>metric_width$} {:<type_width$} Path",
                "",
                "Size",
                "Type",
                rank_width = RANK_PREFIX_WIDTH,
                metric_width = SIZE_COLUMN_WIDTH,
                type_width = TYPE_COLUMN_WIDTH
            );
        }
    }
    println!("{}", "-".repeat(layout.separator_width()));
}

fn print_combined_row(
    layout: TableLayout,
    rank: usize,
    path: &str,
    logical_size: &str,
    disk_usage: &str,
    item_type: &str,
) {
    println!(
        "{}",
        format_combined_row(layout, rank, path, logical_size, disk_usage, item_type)
    );
}

fn print_single_row(layout: TableLayout, rank: usize, path: &str, size: &str, item_type: &str) {
    println!("{}", format_single_row(layout, rank, path, size, item_type));
}

fn format_combined_row(
    layout: TableLayout,
    rank: usize,
    path: &str,
    logical_size: &str,
    disk_usage: &str,
    item_type: &str,
) -> String {
    if layout.full_paths {
        return format!(
            "{rank:2}. {logical_size:>SIZE_COLUMN_WIDTH$} {disk_usage:>SIZE_COLUMN_WIDTH$} {item_type:<TYPE_COLUMN_WIDTH$} {path}"
        );
    }

    format!(
        "{rank:2}. {path:<path_width$} {logical_size:>metric_width$} {disk_usage:>metric_width$} {item_type}",
        path_width = layout.path_width,
        metric_width = SIZE_COLUMN_WIDTH
    )
}

fn format_single_row(
    layout: TableLayout,
    rank: usize,
    path: &str,
    size: &str,
    item_type: &str,
) -> String {
    if layout.full_paths {
        return format!(
            "{rank:2}. {size:>SIZE_COLUMN_WIDTH$} {item_type:<TYPE_COLUMN_WIDTH$} {path}"
        );
    }

    format!(
        "{rank:2}. {path:<path_width$} {size:>metric_width$} {item_type}",
        path_width = layout.path_width,
        metric_width = SIZE_COLUMN_WIDTH
    )
}

fn print_totals(scan_result: &sizr::ScanResult) {
    print_totals_for_mode(
        scan_result.matching_total_size,
        scan_result.matching_logical_size,
        scan_result.matching_disk_usage_size,
        scan_result.size_mode,
        false,
    );
}

fn print_totals_for_mode(
    matching_total_size: u64,
    matching_logical_size: u64,
    matching_disk_usage_size: Option<u64>,
    size_mode: SizeMode,
    leading_newline: bool,
) {
    let prefix = if leading_newline { "\n" } else { "" };

    if size_mode.is_combined() {
        println!(
            "{prefix}Total matching logical size: {}",
            format_human_size(matching_logical_size)
        );
        println!(
            "Total matching disk usage: {}",
            format_human_size(matching_disk_usage_size.unwrap_or(0))
        );
    } else {
        println!(
            "{prefix}{}: {}",
            size_mode.total_label(),
            format_human_size(matching_total_size)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_only_and_dirs_only_conflict() {
        assert!(Args::try_parse_from(["sizr", "--files-only", "--dirs-only"]).is_err());
    }

    #[test]
    fn du_alias_enables_disk_usage() {
        let args = Args::try_parse_from(["sizr", "--du"]).unwrap();

        assert!(args.disk_usage);
    }

    #[test]
    fn both_aliases_enable_combined_mode() {
        let args = Args::try_parse_from(["sizr", "--combined"]).unwrap();

        assert!(args.both);
    }

    #[test]
    fn both_and_disk_usage_conflict() {
        assert!(Args::try_parse_from(["sizr", "--both", "--disk-usage"]).is_err());
    }

    #[test]
    fn no_gitignored_alias_enables_respect_gitignore() {
        let args = Args::try_parse_from(["sizr", "--no-gitignored"]).unwrap();

        assert!(args.respect_gitignore);
    }

    #[test]
    fn full_path_single_metric_rows_keep_metrics_aligned_before_path() {
        let layout = TableLayout::new(SizeMode::DiskUsage, true);
        let short = format_single_row(layout, 1, "/short.bin", "1 B", "FILE");
        let long = format_single_row(
            layout,
            2,
            "/a/very/long/path/that/should/not/shift/the/metric/columns/file.bin",
            "2 B",
            "FILE",
        );

        assert_eq!(short.find("FILE"), long.find("FILE"));
        assert!(short.ends_with("FILE /short.bin"));
        assert!(long
            .ends_with("FILE /a/very/long/path/that/should/not/shift/the/metric/columns/file.bin"));
    }

    #[test]
    fn full_path_combined_rows_keep_metrics_aligned_before_path() {
        let layout = TableLayout::new(SizeMode::Combined, true);
        let short = format_combined_row(layout, 1, "/short.bin", "1 B", "4 KB", "FILE");
        let long = format_combined_row(
            layout,
            2,
            "/a/very/long/path/that/should/not/shift/the/metric/columns/file.bin",
            "2 B",
            "8 KB",
            "FILE",
        );

        assert_eq!(short.find("FILE"), long.find("FILE"));
        assert!(short.ends_with("FILE /short.bin"));
        assert!(long
            .ends_with("FILE /a/very/long/path/that/should/not/shift/the/metric/columns/file.bin"));
    }

    #[test]
    fn truncated_rows_keep_path_before_metric() {
        let layout = TableLayout::new(SizeMode::Logical, false);
        let path = layout.format_path("/a/very/long/path/that/should/be/truncated/file.bin");
        let row = format_single_row(layout, 1, &path, "1 B", "FILE");

        assert!(path.starts_with("..."));
        assert!(row.find(&path).unwrap() < row.find("1 B").unwrap());
    }
}
