use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use humansize::{format_size, DECIMAL};
use anyhow::{Result, Context};

#[derive(Parser)]
#[command(name = "sizr")]
#[command(about = "A CLI tool to explore and list files and folders by size")]
#[command(version = "0.2.0")]
struct Args {
    /// Path to analyze (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: String,

    /// Number of items to display
    #[arg(short, long, default_value = "10")]
    limit: usize,

    /// Show only directories
    #[arg(short, long)]
    dirs_only: bool,

    /// Show only files
    #[arg(short, long)]
    files_only: bool,

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

fn parse_size(size_str: &str) -> Result<u64> {
    if size_str == "0" {
        return Ok(0);
    }
    
    let size_str = size_str.to_uppercase();
    let (number_part, unit_part) = if let Some(pos) = size_str.find(|c: char| c.is_alphabetic()) {
        (&size_str[..pos], &size_str[pos..])
    } else {
        (size_str.as_str(), "")
    };
    
    let number: f64 = number_part.parse()
        .context(format!("Invalid number in size: {}", number_part))?;
    
    let multiplier = match unit_part {
        "" | "B" => 1,
        "KB" => 1_024,
        "MB" => 1_024 * 1_024,
        "GB" => 1_024 * 1_024 * 1_024,
        "TB" => 1_024_u64.pow(4),
        _ => return Err(anyhow::anyhow!("Unknown size unit: {}. Use B, KB, MB, GB, or TB", unit_part)),
    };
    
    Ok((number * multiplier as f64) as u64)
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Determine what to include based on flags
    let (include_files, include_directories) = if args.dirs_only {
        (false, true)
    } else if args.files_only {
        (true, false)
    } else {
        (true, true)  // Default: show both files and directories
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
        println!("Minimum size filter: {}", format_size(min_size_bytes, DECIMAL));
    }
    println!("Scanning files and directories...\n");

    let items = scan_directory(&args.path, include_files, include_directories, min_size_bytes)?;
    
    if items.is_empty() {
        println!("No items found matching the criteria.");
        return Ok(());
    }

    display_results(items, args.limit);
    
    Ok(())
}

fn scan_directory(path: &str, include_files: bool, include_directories: bool, min_size: u64) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    let mut dir_sizes: HashMap<String, u64> = HashMap::new();

    // First pass: collect all file sizes and build directory size map
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        
        if entry_path.is_file() {
            let size = fs::metadata(entry_path)
                .context(format!("Failed to get metadata for {}", entry_path.display()))?
                .len();
            
            // Add file size to all parent directories
            let mut current_path = entry_path.parent();
            while let Some(parent) = current_path {
                let parent_str = parent.to_string_lossy().to_string();
                *dir_sizes.entry(parent_str).or_insert(0) += size;
                current_path = parent.parent();
            }

            // Add file to items if files are included and size meets minimum requirement
            if include_files && size >= min_size {
                items.push(Item {
                    path: entry_path.to_string_lossy().to_string(),
                    size,
                    is_directory: false,
                });
            }
        }
    }

    // Second pass: add directories if requested
    if include_directories {
        for entry in WalkDir::new(path).min_depth(1).into_iter().filter_map(|e| e.ok()) {
            let entry_path = entry.path();
            
            if entry_path.is_dir() {
                let path_str = entry_path.to_string_lossy().to_string();
                let size = dir_sizes.get(&path_str).copied().unwrap_or(0);
                
                // Add directory to items only if size meets minimum requirement
                if size >= min_size {
                    items.push(Item {
                        path: path_str,
                        size,
                        is_directory: true,
                    });
                }
            }
        }
    }

    // Sort by size (largest first)
    items.sort_by(|a, b| b.size.cmp(&a.size));
    
    Ok(items)
}

fn display_results(items: Vec<Item>, limit: usize) {
    let display_count = std::cmp::min(items.len(), limit);
    
    println!("Top {} largest items:", display_count);
    println!("{:<50} {:>12} {}", "Path", "Size", "Type");
    println!("{}", "-".repeat(70));

    for (index, item) in items.iter().take(limit).enumerate() {
        let size_str = format_size(item.size, DECIMAL);
        let type_str = if item.is_directory { "DIR" } else { "FILE" };
        let path_display = if item.path.len() > 47 {
            format!("...{}", &item.path[item.path.len() - 44..])
        } else {
            item.path.clone()
        };
        
        println!("{:2}. {:<47} {:>12} {}", 
                 index + 1, 
                 path_display, 
                 size_str, 
                 type_str);
    }

    if items.len() > limit {
        println!("\n... and {} more items", items.len() - limit);
    }

    // Calculate total size based only on files to avoid double-counting
    let total_size: u64 = items.iter()
        .filter(|item| !item.is_directory)
        .map(|item| item.size)
        .sum();
    println!("\nTotal size analyzed: {}", format_size(total_size, DECIMAL));
}
