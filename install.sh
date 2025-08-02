#!/bin/bash

# sizr installation script

echo "Building sizr..."
cargo build --release

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    echo ""
    echo "The binary is available at: target/release/sizr"
    echo ""
    echo "To install globally, you can run:"
    echo "  sudo cp target/release/sizr /usr/local/bin/"
    echo "  # or"
    echo "  cargo install --path ."
    echo ""
    echo "Usage examples:"
    echo "  ./target/release/sizr                    # Analyze current directory"
    echo "  ./target/release/sizr --limit 20         # Show top 20 items"
    echo "  ./target/release/sizr --dirs-only        # Show only directories"
    echo "  ./target/release/sizr --path ~/Downloads  # Analyze specific folder"
else
    echo "❌ Build failed!"
    exit 1
fi
