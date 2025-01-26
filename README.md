# LeftRight - Fast Image Sorter

LeftRight is a lightning-fast, keyboard-driven image sorting tool built in Rust using the egui framework. It allows you to quickly categorize images into custom folders with smooth animations and an intuitive interface.

https://github.com/user-attachments/assets/c4748c61-ed19-4b9c-852c-7fb3954f1cab

## Features

- Quick image sorting with keyboard shortcuts
- Visual feedback with smooth animations
- Concurrent image loading for fast startup
- Support for multiple image formats (JPG, PNG, GIF, WebP)
- Undo functionality
- Real-time progress tracking

## Installation

```bash
cargo install leftright
```

## Usage

```bash
# Sort images in current directory
leftright

# Sort images in specific directory
leftright -d /path/to/images

# Get help
leftright --help
```

## Keyboard Shortcuts

- `←` - Move image to left category
- `→` - Move image to right category
- `↑` - Move image to up category
- `↓` - Move image to down category
- `Ctrl+Z` - Undo last move

## Building from Source

```bash
git clone https://github.com/yourusername/leftright
cd leftright
cargo build --release
```

The binary will be available in `target/release/leftright`

## License

MIT
