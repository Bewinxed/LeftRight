# LeftRight - Fast Image Sorter

LeftRight is a lightning-fast, keyboard-driven image sorting tool built in Rust using the egui framework. It allows you to quickly categorize images into custom folders with smooth animations and an intuitive interface.

![LeftRight Screenshot](screenshot.png)

## Features

- **Fast Concurrent Image Loading**: Loads multiple images simultaneously for quick startup
- **Custom Categories**: Define 1-4 categories for sorting your images
- **Keyboard Shortcuts**: Quick sorting using arrow keys
- **Smooth Animations**: Visual feedback with animated transitions
- **Progress Tracking**: Real-time progress indication during image loading
- **Undo Support**: Easily undo sorting mistakes with Ctrl+Z

## Usage

1. Run the application in a directory containing images
2. Enter your category names (1-4), separated by commas (e.g., "good,bad" or "keep,maybe,delete")
3. Use arrow keys to sort images:
   - ← Left Arrow: First category
   - → Right Arrow: Second category
   - ↑ Up Arrow: Third category
   - ↓ Down Arrow: Fourth category
   - Ctrl+Z: Undo last move

## Installation

### Prerequisites
- Rust 1.75 or higher
- Cargo package manager

### Building from source
```bash
git clone https://github.com/yourusername/leftright.git
cd leftright
cargo build --release
```

The executable will be available in `target/release/`

## Technical Details

- Built with Rust and egui for the UI
- Uses tokio for async image loading
- Supports common image formats (JPG, PNG, GIF, WebP)
- Multi-threaded image processing
- Efficient memory management

## License

MIT License - See LICENSE file for details

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
