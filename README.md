# sizr

A CLI tool written in Rust to explore and list files and folders by size.

## Features

- Analyze logical file sizes in any directory
- Sort results by size (largest first)
- Configurable number of results to display
- Option to show only files, only directories, or both
- Human-readable file sizes
- Machine-readable JSON output
- Optional disk-usage mode for allocated filesystem blocks
- Optional gitignore-aware scanning
- Minimum size filtering with flexible units (B, KB, MB, GB, TB)
- Full path display option for complete file paths
- Execution timing to track scan performance
- Parallel filesystem traversal
- Unicode-safe path handling for international file names
- Cross-platform support for logical-size scans

## Installation

Make sure you have Rust installed on your system. If not, install it from [rustup.rs](https://rustup.rs/).

Install directly into Cargo's bin directory:

```bash
cargo install --path .
```

Alternatively, build the binary and install it to `/usr/local/bin`:

```bash
cargo build --release
sudo install -m 755 target/release/sizr /usr/local/bin/sizr
```

## Usage

```bash
sizr [OPTIONS]
```

By default, `sizr` scans the current directory, ranks files and directories by logical size, and prints the top 10 matching items.
Options can be combined, such as `sizr -p ~/Downloads -f -m 50MB -P`.

Run `sizr --help` for the generated command-line help.

### Options

- `-p, --path <PATH>`: Path to analyze (defaults to current directory)
- `-l, --limit <LIMIT>`: Number of items to display (default: 10)
- `-m, --min-size <MIN_SIZE>`: Minimum selected size metric to display (e.g., 1MB, 500KB, 2GB). Default is 0 (show all)
- `-d, --dirs-only`: Show only directories
- `-f, --files-only`: Show only files
- `-P, --full-paths`: Display full paths instead of truncating them
- `--disk-usage`, `--du`: Rank and filter by allocated disk usage instead of logical file size
- `--both`: Show logical size and disk usage side by side, ranked and filtered by logical size
- `--respect-gitignore`: Skip files ignored by `.gitignore`, `.git/info/exclude`, or global gitignore rules
- `--json`: Output machine-readable JSON instead of the human table
- `-h, --help`: Show help information
- `-V, --version`: Show version information

For compatibility, `--combined` and `--compare` remain accepted as hidden aliases for `--both`, and `--no-gitignored` remains accepted as a hidden alias for `--respect-gitignore`.

### Size Values

The `--min-size` argument accepts human-readable size formats:
- `500` or `500B` - 500 bytes
- `1KB` - 1 kilobyte (1,024 bytes)
- `1MB` - 1 megabyte (1,048,576 bytes)
- `2GB` - 2 gigabytes
- `1TB` - 1 terabyte

## Size Semantics

By default, `sizr` reports logical file size in bytes, using filesystem metadata for regular files. This default is not equivalent to `du`.

- File rows show the file's logical byte length.
- Directory rows show the sum of contained regular file logical sizes, used for ranking.
- The footer, `Total matching file size`, counts matching file rows once and does not add directory rows on top of their contents.
- `--min-size` applies to file rows by individual file size and to directory rows by aggregate directory size.
- Symlinks are not followed or counted by default.

With `--disk-usage` or `--du`, `sizr` uses allocated filesystem blocks instead. This answers the `du`-style question: how much disk space is actually allocated for matching files. Directory rows and totals are then based on contained allocated disk usage. Disk-usage mode is supported on Unix-like platforms.

The default table shows one selected metric so rankings and totals stay compact. Use `--both` to show logical size and disk usage side by side. Combined mode ranks and filters by logical size, then reports both logical and allocated totals.

In JSON output, single-metric modes use `size_bytes` / `size_human` and `total_matching_size_*`. Combined mode omits those generic selected-size fields and instead emits explicit `logical_size_*` and `disk_usage_*` fields for items and totals.

In disk-usage mode on Unix-like platforms, hardlinked files are counted once per device/inode pair. Duplicate hardlink paths are omitted from the disk-usage listing instead of showing as `0 B` rows. Totals still count the allocated blocks once.

By default, `sizr` scans what is actually on disk, including files ignored by Git. With `--respect-gitignore`, it skips files ignored by `.gitignore`, `.git/info/exclude`, or global gitignore rules. This is useful when you want the “what could matter to this repo?” view instead of cache/build-output growth.

## Library API

The crate also exposes the scanner and JSON primitives for Rust callers:

```rust
use sizr::{scan_directory, ScanOptions};

fn main() -> anyhow::Result<()> {
    let result = scan_directory(".", ScanOptions::new(true, true, 0))?;
    for item in result.items {
        println!("{}\t{}", item.size, item.path);
    }

    Ok(())
}
```

## Examples

```bash
# See the largest files and directories in the current folder
sizr

# Inspect Downloads with a larger result set
sizr -p ~/Downloads -l 20

# Find large files with copy-friendly full paths
sizr -p ~/Downloads -f -m 50MB -P

# Rank directories by contained logical size
sizr -d -l 20

# Rank by allocated disk usage instead of logical size
sizr -p ~/Downloads --du -l 20

# Compare logical size and allocated disk usage
sizr -p ~/Downloads --both -l 20

# Scan a repository while skipping ignored build/cache output
sizr -p . --respect-gitignore -l 20

# Emit JSON for scripts or CI checks
sizr -p ~/Downloads -f -m 50MB --json

# Find very large files under a data directory
sizr -p /path/to/data -f -m 1GB -l 20
```

## Output Format

The tool displays results in a formatted table showing:
1. Rank number
2. Path (truncated by default, full path with `--full-paths`)
3. Selected size metric in human-readable format (DIR rows show contained file bytes or allocated bytes for ranking)
4. Type (FILE or DIR)
5. Total matching bytes for the selected metric, counted once without adding directory rows on top of their contents
6. Execution timing information

With `--full-paths`, `sizr` prints size and type before the path so long untruncated paths do not push the metric columns out of alignment.

Example output:
```
Analyzing path: /Users/username/Documents
Scanning files and directories...

Top 10 largest items:
Path                                              Size Type
----------------------------------------------------------------------
 1. ...username/Documents/large-video.mp4          2.1 GB FILE
 2. ...username/Documents/Photos                   1.8 GB DIR
 3. ...username/Documents/archive.zip              500 MB FILE
 ...

Total matching file size: 15.2 GB
Scan completed in 245.67ms
```

With `--full-paths`:
```
            Size Type Path
----------------------------------------------------------------------------------------------------
 1.       2.1 GB FILE /Users/username/Documents/Videos/vacation-2023.mp4
 2.       1.8 GB DIR  /Users/username/Documents/Photos
 ...

Total matching file size: 15.2 GB
Scan completed in 245.67ms
```

With `--both`:
```
Path                                            Logical   Disk Usage Type
------------------------------------------------------------------------------
 1. ...username/Documents/sparse.img             10.0 GB      10.2 MB FILE
 2. ...username/Documents/Photos                  1.8 GB       1.9 GB DIR
 ...

Total matching logical size: 15.2 GB
Total matching disk usage: 7.4 GB
Scan completed in 245.67ms
```

With `--json`, stdout contains only JSON:
```json
{
  "schema_version": 4,
  "root_path": "/Users/username/Documents",
  "size_mode": "logical",
  "size_semantics": "logical_file_size_bytes",
  "directory_size_semantics": "sum_of_contained_logical_file_size_bytes",
  "symlinks_followed": false,
  "gitignore_respected": false,
  "min_size_bytes": 0,
  "limit": 1,
  "displayed_count": 1,
  "total_items_count": 2,
  "total_matching_size_bytes": 1500000,
  "total_matching_size_human": "1.5 MB",
  "elapsed_ms": 245.67,
  "items": [
    {
      "rank": 1,
      "path": "/Users/username/Documents/large-video.mp4",
      "type": "file",
      "size_bytes": 1500000,
      "size_human": "1.5 MB"
    }
  ],
  "warnings": {
    "count": 0,
    "messages": []
  }
}
```

## License

This project is open source and available under the MIT License.
