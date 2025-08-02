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
#[command(version = "0.1.0")]
struct Args {
    /// Path to analyze (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    path: String,

    /// Number of items to display
    #[arg(short, long, default_value = "10")]
    limit: usize,

    /// Include files in the listing (default: true)
    #[arg(long, default_value = "true")]
    files: bool,

    /// Include directories in the listing (default: true)
    #[arg(long, default_value = "true")]
    directories: bool,

    /// Show only directories (shorthand for --files=false --directories=true)
    #[arg(short, long)]
    dirs_only: bool,

    /// Show only files (shorthand for --files=true --directories=false)
    #[arg(short, long)]
    files_only: bool,
}

#[derive(Debug, Clone)]
struct Item {
    path: String,
    size: u64,
    is_directory: bool,
}

fn main() -> Result<()> {
    let mut args = Args::parse();

    // Handle shorthand flags
    if args.dirs_only {
        args.files = false;
        args.directories = true;
    } else if args.files_only {
        args.files = true;
        args.directories = false;
    }

    let path = Path::new(&args.path);
    if !path.exists() {
        eprintln!("Error: Path '{}' does not exist", args.path);
        std::process::exit(1);
    }

    println!("Analyzing path: {}", path.display());
    println!("Scanning files and directories...\n");

    let items = scan_directory(&args.path, args.files, args.directories)?;
    
    if items.is_empty() {
        println!("No items found matching the criteria.");
        return Ok(());
    }

    display_results(items, args.limit);
    
    Ok(())
}

fn scan_directory(path: &str, include_files: bool, include_directories: bool) -> Result<Vec<Item>> {
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

            // Add file to items if files are included
            if include_files {
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
                
                items.push(Item {
                    path: path_str,
                    size,
                    is_directory: true,
                });
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

    let total_size: u64 = items.iter().map(|item| item.size).sum();
    println!("\nTotal size analyzed: {}", format_size(total_size, DECIMAL));
}
