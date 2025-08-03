# sizr

A CLI tool written in Rust to explore and list files and folders by size.

## Features

- Analyze files and folders in any directory
- Sort results by size (largest first)
- Configurable number of results to display
- Option to show only files, only directories, or both
- Human-readable file sizes
- Minimum size filtering with flexible units (B, KB, MB, GB, TB)
- Full path display option for complete file paths
- Execution timing to track scan performance
- Unicode-safe path handling for international file names
- Cross-platform support

## Installation

Make sure you have Rust installed on your system. If not, install it from [rustup.rs](https://rustup.rs/).

```bash
# Clone or navigate to the project directory
cd sizr

# Build the project
cargo build --release

# The binary will be available at target/release/sizr
```

## Usage

```bash
# Analyze current directory, show top 10 items
sizr

# Analyze specific directory
sizr --path /path/to/directory
# or using short form
sizr -p /path/to/directory

# Show top 20 items
sizr --limit 20
# or using short form
sizr -l 20

# Show only directories
sizr --dirs-only
# or using short form
sizr -d

# Show only files
sizr --files-only
# or using short form
sizr -f

# Show only files larger than 1MB
sizr --min-size 1MB --files-only
# or using short forms
sizr -m 1MB -f

# Show items larger than 500KB
sizr --min-size 500KB
# or using short form
sizr -m 500KB

# Display full paths instead of truncating them
sizr --full-paths
# or using short form
sizr -P

# Combine options: show only large files with full paths
sizr --files-only --min-size 10MB --full-paths
# or using short forms
sizr -f -m 10MB -P

# Analyze specific path with custom limit and size filter
sizr --path /Users/username/Documents --limit 15 --min-size 2MB
# or using short forms
sizr -p /Users/username/Documents -l 15 -m 2MB
```

### Command Line Options

- `-p, --path <PATH>`: Path to analyze (defaults to current directory)
- `-l, --limit <LIMIT>`: Number of items to display (default: 10)
- `-m, --min-size <MIN_SIZE>`: Minimum size to display (e.g., 1MB, 500KB, 2GB). Default is 0 (show all)
- `-d, --dirs-only`: Show only directories
- `-f, --files-only`: Show only files
- `-P, --full-paths`: Display full paths instead of truncating them
- `-h, --help`: Show help information
- `-V, --version`: Show version information

#### Size Format Examples

The `--min-size` argument accepts human-readable size formats:
- `500` or `500B` - 500 bytes
- `1KB` - 1 kilobyte (1,024 bytes)
- `1MB` - 1 megabyte (1,048,576 bytes)
- `2GB` - 2 gigabytes
- `1TB` - 1 terabyte

## Examples

```bash
# Show top 5 largest files and directories in current folder
sizr --limit 5
# or using short form
sizr -l 5

# Show top 10 largest directories only
sizr --dirs-only
# or using short form
sizr -d

# Analyze Downloads folder and show top 20 items
sizr --path ~/Downloads --limit 20
# or using short forms
sizr -p ~/Downloads -l 20

# Show only files in a specific directory
sizr --path /var/log --files-only --limit 15
# or using short forms
sizr -p /var/log -f -l 15

# Analyze large directories with full path display
sizr --path ~/Downloads --full-paths --limit 5
# or using short forms
sizr -p ~/Downloads -P -l 5

# Find large files across the system with size filtering
sizr --path / --files-only --min-size 1GB --limit 20
# or using short forms
sizr -p / -f -m 1GB -l 20
```

## Output Format

The tool displays results in a formatted table showing:
1. Rank number
2. Path (truncated by default, full path with `--full-paths`)
3. Size in human-readable format
4. Type (FILE or DIR)
5. Execution timing information

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

Total size analyzed: 15.2 GB
Scan completed in 245.67ms
```

With `--full-paths`:
```
Path                                                                            Size Type
----------------------------------------------------------------------------------------------------
 1. /Users/username/Documents/Videos/vacation-2023.mp4                          2.1 GB FILE
 2. /Users/username/Documents/Photos                                            1.8 GB DIR
 ...

Total size analyzed: 15.2 GB
Scan completed in 245.67ms
```

## Dependencies

- `clap`: Command-line argument parsing
- `walkdir`: Recursive directory traversal
- `humansize`: Human-readable size formatting
- `anyhow`: Error handling

## License

This project is open source and available under the MIT License.
