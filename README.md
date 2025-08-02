# sizr

A CLI tool written in Rust to explore and list files and folders by size.

## Features

- Analyze files and folders in any directory
- Sort results by size (largest first)
- Configurable number of results to display
- Option to show only files, only directories, or both
- Human-readable file sizes
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

# Show top 20 items
sizr --limit 20

# Show only directories
sizr --dirs-only

# Show only files
sizr --files-only

# Analyze specific path with custom limit
sizr --path /Users/username/Documents --limit 15
```

### Command Line Options

- `-p, --path <PATH>`: Path to analyze (defaults to current directory)
- `-l, --limit <LIMIT>`: Number of items to display (default: 10)
- `--files <BOOL>`: Include files in the listing (default: true)
- `--directories <BOOL>`: Include directories in the listing (default: true)
- `-d, --dirs-only`: Show only directories
- `-f, --files-only`: Show only files
- `-h, --help`: Show help information
- `-V, --version`: Show version information

## Examples

```bash
# Show top 5 largest files and directories in current folder
sizr --limit 5

# Show top 10 largest directories only
sizr --dirs-only

# Analyze Downloads folder and show top 20 items
sizr --path ~/Downloads --limit 20

# Show only files in a specific directory
sizr --path /var/log --files-only --limit 15
```

## Output Format

The tool displays results in a formatted table showing:
1. Rank number
2. Path (truncated if too long)
3. Size in human-readable format
4. Type (FILE or DIR)

Example output:
```
Analyzing path: /Users/username/Documents
Scanning files and directories...

Top 10 largest items:
Path                                              Size Type
----------------------------------------------------------------------
 1. /Users/username/Documents/large-video.mp4      2.1 GB FILE
 2. /Users/username/Documents/Photos                1.8 GB DIR
 3. /Users/username/Documents/archive.zip           500 MB FILE
 ...
```

## Dependencies

- `clap`: Command-line argument parsing
- `walkdir`: Recursive directory traversal
- `humansize`: Human-readable size formatting
- `anyhow`: Error handling

## License

This project is open source and available under the MIT License.
