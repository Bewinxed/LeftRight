use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Runtime;

// Background worker for image loading
struct ImageLoader {
    runtime: Runtime,
}

impl ImageLoader {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4) // Use 4 worker threads
            .enable_all()
            .build()
            .unwrap();

        Self { runtime }
    }

    async fn load_image(
        path: PathBuf,
        ctx: egui::Context,
    ) -> Option<(PathBuf, egui::TextureHandle)> {
        let path_for_image = path.clone();

        // Move image loading to a blocking task with optimized settings
        let image_result = tokio::task::spawn_blocking(move || {
            image::io::Reader::open(&path_for_image)
                .ok()?
                .with_guessed_format()
                .ok()?
                .decode()
                .ok()
        })
        .await
        .ok()??;

        let max_dimension = 1200.0;
        let resized = if image_result.width() as f32 > max_dimension
            || image_result.height() as f32 > max_dimension
        {
            let scale = max_dimension / image_result.width().max(image_result.height()) as f32;
            image_result.resize(
                (image_result.width() as f32 * scale) as u32,
                (image_result.height() as f32 * scale) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            image_result
        };

        let size = [resized.width() as _, resized.height() as _];
        let image_buffer = resized.to_rgba8();

        let texture = ctx.load_texture(
            path.to_string_lossy().to_string(),
            egui::ImageData::Color(Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                size,
                &image_buffer,
            ))),
            egui::TextureOptions::default(),
        );

        Some((path, texture))
    }
}

#[derive(Clone)]
struct Animation {
    path: PathBuf,
    start_pos: egui::Pos2,
    end_pos: egui::Pos2,
    start_time: Instant,
    duration: f32,
    start_scale: f32,
    end_scale: f32,
}

struct MoveOperation {
    from: PathBuf,
    to: PathBuf,
    timestamp: Instant,
}

struct CategoryBucket {
    files: Vec<PathBuf>,
    rect: egui::Rect,
    stack_offset: f32,
    next_stack_position: f32, // Add this field to track where the next card should go
}

struct ImageSorter {
    images: Vec<PathBuf>,
    categories: Vec<String>,
    category_buckets: HashMap<String, CategoryBucket>,
    current_image: Option<usize>,
    textures: HashMap<PathBuf, egui::TextureHandle>,
    animations: Vec<Animation>,
    moves: Vec<MoveOperation>,
    setup_done: bool,
    input_categories: String,
    last_image_pos: Option<egui::Pos2>,
    loading_progress: f32,
    is_loading: bool,
    loader: ImageLoader,
    pending_loads: Vec<PathBuf>,
    texture_rx: Receiver<(PathBuf, egui::TextureHandle)>,
    texture_tx: Sender<(PathBuf, egui::TextureHandle)>,
    total_images_to_load: usize,
    pending_moves: Vec<PendingMove>,
}

#[derive(Clone)]
struct PendingMove {
    from: PathBuf,
    to: PathBuf,
}

impl ImageSorter {
    fn new() -> Self {
        let (texture_tx, texture_rx) = channel();
        Self {
            images: Vec::new(),
            categories: Vec::new(),
            category_buckets: HashMap::new(),
            current_image: None,
            textures: HashMap::new(),
            animations: Vec::new(),
            moves: Vec::new(),
            setup_done: false,
            input_categories: String::new(),
            last_image_pos: None,
            loading_progress: 0.0,
            is_loading: false,
            loader: ImageLoader::new(),
            pending_loads: Vec::new(),
            texture_rx,
            texture_tx,
            total_images_to_load: 0, // Add this field
            pending_moves: Vec::new(),
        }
    }

    fn start_background_loading(&mut self, ctx: &egui::Context) {
        self.images = std::fs::read_dir(".")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_lowercase());
                matches!(
                    ext.as_deref(),
                    Some("jpg" | "jpeg" | "png" | "gif" | "webp")
                )
            })
            .map(|entry| entry.path())
            .collect();

        println!("Found {} images", self.images.len());

        if !self.images.is_empty() {
            self.is_loading = true;
            self.loading_progress = 0.0;
            self.total_images_to_load = self.images.len();

            // Start concurrent loading of ALL images
            let chunk_size = 4; // Load 4 images at a time
            for chunk in self.images.chunks(chunk_size) {
                for path in chunk {
                    if self.textures.contains_key(path) || self.pending_loads.contains(path) {
                        continue;
                    }

                    println!("Starting to load image: {}", path.display());
                    self.pending_loads.push(path.clone());

                    let ctx = ctx.clone();
                    let tx = self.texture_tx.clone();
                    let path_clone = path.clone();

                    self.loader.runtime.spawn(async move {
                        if let Some((loaded_path, texture)) =
                            ImageLoader::load_image(path_clone, ctx.clone()).await
                        {
                            println!("Finished loading image: {}", loaded_path.display());
                            let _ = tx.send((loaded_path, texture));
                            ctx.request_repaint();
                        }
                    });
                }
            }
        }
    }

    fn ensure_textures_loaded(&mut self, current_idx: usize, ctx: &egui::Context) {
        // Preload current and next few images
        for idx in current_idx..=(current_idx + 2).min(self.images.len() - 1) {
            let path = match self.images.get(idx) {
                // Fixed: use idx instead of current_idx
                Some(p) => p.clone(),
                None => continue, // Changed: continue instead of return to process all indices
            };

            // Don't reload if already loaded or pending
            if self.textures.contains_key(&path) || self.pending_loads.contains(&path) {
                continue; // Changed: continue instead of return to process all indices
            }

            println!("Starting to load image: {}", path.display());

            // Add to pending loads
            self.pending_loads.push(path.clone());

            // Start async load
            let ctx = ctx.clone();
            let tx = self.texture_tx.clone();

            self.loader.runtime.spawn(async move {
                if let Some((loaded_path, texture)) =
                    ImageLoader::load_image(path, ctx.clone()).await
                {
                    println!("Finished loading image: {}", loaded_path.display());
                    let _ = tx.send((loaded_path, texture));
                    ctx.request_repaint();
                }
            });
        }
    }

    fn revert_last_move(&mut self) {
        if let Some(last_move) = self.moves.pop() {
            // Clone paths for both async operation and UI update
            let from_async = last_move.from.clone();
            let to_async = last_move.to.clone();
            let from_ui = last_move.from.clone();
            let to_ui = last_move.to;

            // Spawn file operation in background
            self.loader.runtime.spawn(async move {
                if let Err(e) = tokio::fs::rename(&to_async, &from_async).await {
                    eprintln!("Failed to revert move: {}", e);
                }
            });

            // Update UI state immediately
            if let Some(current_idx) = self.current_image {
                self.images.insert(current_idx, from_ui.clone());
            } else {
                self.images.push(from_ui.clone());
                self.current_image = Some(self.images.len() - 1);
            }

            // Keep the texture around since we'll need it again
            if let Some(texture) = self.textures.remove(&to_ui) {
                self.textures.insert(from_ui, texture);
            }
        }
    }

    fn setup_categories(&mut self, ctx: &egui::Context) {
        for category in &self.categories {
            std::fs::create_dir_all(category).unwrap();
            self.category_buckets.insert(
                category.clone(),
                CategoryBucket {
                    files: Vec::new(),
                    rect: egui::Rect::NOTHING,
                    stack_offset: 3.0,
                    next_stack_position: 0.0, // Initialize at 0
                },
            );
        }

        self.refresh_images(ctx);
    }

    fn refresh_images(&mut self, ctx: &egui::Context) {
        self.images = std::fs::read_dir(".")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.to_lowercase());
                matches!(
                    ext.as_deref(),
                    Some("jpg" | "jpeg" | "png" | "gif" | "webp")
                )
            })
            .map(|entry| entry.path())
            .collect();

        println!("Found {} images", self.images.len());

        if !self.images.is_empty() {
            self.current_image = Some(0);
            self.is_loading = true;
            self.loading_progress = 0.0;
            self.total_images_to_load = self.images.len();

            // Start concurrent loading of ALL images
            let chunk_size = 4; // Load 4 images at a time
            for chunk in self.images.chunks(chunk_size) {
                for path in chunk {
                    // Skip if already loaded or pending
                    if self.textures.contains_key(path) || self.pending_loads.contains(path) {
                        continue;
                    }

                    println!("Starting to load image: {}", path.display());
                    self.pending_loads.push(path.clone());

                    // Start async load
                    let ctx = ctx.clone();
                    let tx = self.texture_tx.clone();
                    let path_clone = path.clone();

                    self.loader.runtime.spawn(async move {
                        if let Some((loaded_path, texture)) =
                            ImageLoader::load_image(path_clone, ctx.clone()).await
                        {
                            println!("Finished loading image: {}", loaded_path.display());
                            let _ = tx.send((loaded_path, texture));
                            ctx.request_repaint();
                        }
                    });
                }
            }
        }

        // Refresh category buckets
        for (category, bucket) in self.category_buckets.iter_mut() {
            bucket.files = std::fs::read_dir(category)
                .unwrap_or_else(|_| std::fs::read_dir(".").unwrap())
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect();
        }
    }

    fn draw_buckets(&mut self, ui: &mut egui::Ui, center: egui::Pos2, panel_size: egui::Vec2) {
        let bucket_size = egui::vec2(100.0, 150.0);
        let spacing = panel_size.x * 0.25;
        let directions = ["←", "→", "↑", "↓"];
        let bucket_positions = [
            center + egui::vec2(-spacing, 0.0),
            center + egui::vec2(spacing, 0.0),
            center + egui::vec2(0.0, -spacing),
            center + egui::vec2(0.0, spacing),
        ];

        for (i, category) in self.categories.iter().enumerate() {
            if let Some(bucket) = self.category_buckets.get_mut(category) {
                bucket.rect = egui::Rect::from_center_size(bucket_positions[i], bucket_size);

                // Draw bucket background
                ui.painter()
                    .rect_filled(bucket.rect, 5.0, egui::Color32::from_gray(40));

                // Draw stacked cards in bucket with proper offset
                let max_visible_cards = 5;
                let visible_files: Vec<_> = bucket.files.iter().take(max_visible_cards).collect();

                for (stack_idx, file_path) in visible_files.iter().enumerate().rev() {
                    if let Some(texture) = self.textures.get(*file_path) {
                        let offset = stack_idx as f32 * bucket.stack_offset;
                        let card_rect = egui::Rect::from_center_size(
                            bucket.rect.center() + egui::vec2(offset, offset),
                            bucket_size * 0.8,
                        );

                        // Draw card shadow
                        ui.painter().rect_filled(
                            card_rect.translate(egui::vec2(2.0, 2.0)),
                            3.0,
                            egui::Color32::from_black_alpha(40),
                        );

                        // Draw card
                        ui.painter().image(
                            texture.id(),
                            card_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                }

                // Draw bucket label
                ui.painter().text(
                    bucket.rect.center() + egui::vec2(0.0, bucket_size.y * 0.4),
                    egui::Align2::CENTER_CENTER,
                    format!(
                        "{} {}\n{} files",
                        directions[i],
                        category,
                        bucket.files.len()
                    ),
                    egui::FontId::proportional(16.0),
                    egui::Color32::WHITE,
                );
            }
        }
    }

    fn update_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Process completed texture loads
        while let Ok((path, texture)) = self.texture_rx.try_recv() {
            self.textures.insert(path.clone(), texture);
            self.pending_loads.retain(|p| p != &path);

            if self.is_loading {
                self.loading_progress =
                    (self.textures.len() as f32) / (self.total_images_to_load as f32);
                if self.textures.len() >= self.total_images_to_load {
                    self.is_loading = false;
                }
            }
        }

        if self.is_loading {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 2.0 - 20.0);
                ui.add(egui::ProgressBar::new(self.loading_progress).show_percentage());
                ui.label(format!(
                    "Loading images... ({}/{})",
                    self.textures.len(),
                    self.total_images_to_load
                ));
            });
            return;
        }

        let panel_size = ui.available_size();
        let center = ui.available_rect_before_wrap().center();

        // Draw buckets first (background layer)
        self.draw_buckets(ui, center, panel_size);

        // Draw current image (middle layer) only if not animating
        if self.animations.is_empty() {
            if let Some(current_idx) = self.current_image {
                if let Some(path) = self.images.get(current_idx) {
                    if let Some(texture) = self.textures.get(path) {
                        let image_size = {
                            let aspect = texture.aspect_ratio();
                            let height = panel_size.y * 0.4;
                            egui::vec2(height * aspect, height)
                        };

                        let image_rect = egui::Rect::from_center_size(center, image_size);
                        ui.painter().image(
                            texture.id(),
                            image_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );

                        self.last_image_pos = Some(image_rect.center());
                    }
                }
            }
        }

        // Draw animations (top layer)
        self.update_animations(ui, panel_size);

        // Handle keyboard input
        let input = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Z) && i.modifiers.ctrl,
            )
        });

        match input {
            (true, _, _, _, _) if !self.categories.is_empty() => self.move_image(0, center, ctx),
            (_, true, _, _, _) if self.categories.len() > 1 => self.move_image(1, center, ctx),
            (_, _, true, _, _) if self.categories.len() > 2 => self.move_image(2, center, ctx),
            (_, _, _, true, _) if self.categories.len() > 3 => self.move_image(3, center, ctx),
            (_, _, _, _, true) => self.revert_last_move(),
            _ => {}
        }

        // Request repaint if there are active animations
        if !self.animations.is_empty() {
            ctx.request_repaint();
        }
    }

    fn move_image(&mut self, direction: usize, center_pos: egui::Pos2, ctx: &egui::Context) {
        if let Some(current_idx) = self.current_image {
            if self.images.is_empty() || current_idx >= self.images.len() {
                return;
            }

            if direction >= self.categories.len() {
                return;
            }

            let from = self.images[current_idx].clone();
            let category = &self.categories[direction].clone();
            let to = Path::new(category).join(from.file_name().unwrap());

            // Create animation BEFORE moving the file
            if let Some(bucket) = self.category_buckets.get_mut(category) {
                let start_pos = self.last_image_pos.unwrap_or(center_pos);
                let end_pos = bucket.rect.center();

                println!(
                    "Creating animation: start={:?}, end={:?}",
                    start_pos, end_pos
                );

                // Ensure we have the texture before creating the animation
                if let Some(_texture) = self.textures.get(&from) {
                    let animation = Animation {
                        path: from.clone(),
                        start_pos,
                        end_pos,
                        start_time: Instant::now(),
                        duration: 0.5,    // Faster animation
                        start_scale: 1.2, // Start larger
                        end_scale: 0.6,   // End smaller
                    };
                    self.animations.push(animation);

                    // Add to pending moves instead of moving immediately
                    self.pending_moves.push(PendingMove {
                        from: from.clone(),
                        to: to.clone(),
                    });

                    println!("Added animation and pending move");
                }

                // Update bucket's next stack position
                bucket.next_stack_position += bucket.stack_offset;
                if bucket.next_stack_position > 15.0 {
                    bucket.next_stack_position = 0.0;
                }
            }

            // Move file in background
            let from_clone = from.clone();
            let to_clone = to.clone();
            self.loader.runtime.spawn(async move {
                if let Err(e) = tokio::fs::rename(&from_clone, &to_clone).await {
                    eprintln!("Failed to move file: {}", e);
                }
            });

            // Record the move operation
            self.moves.push(MoveOperation {
                from: from.clone(),
                to,
                timestamp: Instant::now(),
            });

            // Remove from images list but keep texture until animation completes
            self.images.remove(current_idx);
            if !self.images.is_empty() {
                self.current_image = Some(current_idx.min(self.images.len() - 1));
            } else {
                self.current_image = None;
            }
        }
    }

    fn update_animations(&mut self, ui: &mut egui::Ui, panel_size: egui::Vec2) {
        let mut completed_animations = Vec::new();

        self.animations.retain_mut(|anim| {
            let elapsed = anim.start_time.elapsed().as_secs_f32();
            let progress = (elapsed / anim.duration).min(1.0);

            // Smooth easing function
            let eased_progress = 1.0 - (1.0 - progress).powi(3);

            // Calculate current position with Y animation
            let current_pos = egui::Pos2 {
                x: anim.start_pos.x + (anim.end_pos.x - anim.start_pos.x) * eased_progress,
                y: anim.start_pos.y + (anim.end_pos.y - anim.start_pos.y) * eased_progress,
            };

            let current_scale =
                anim.start_scale + (anim.end_scale - anim.start_scale) * eased_progress;

            if let Some(texture) = self.textures.get(&anim.path) {
                // Calculate size based on the original image aspect ratio
                let aspect = texture.aspect_ratio();
                let base_height = panel_size.y * 0.4;
                let base_size = egui::vec2(base_height * aspect, base_height);
                let size = base_size * current_scale;

                // Draw shadow and image
                let shadow_rect =
                    egui::Rect::from_center_size(current_pos + egui::vec2(2.0, 2.0), size);
                ui.painter().rect_filled(
                    shadow_rect,
                    3.0,
                    egui::Color32::from_black_alpha((40.0 * (1.0 - progress)) as u8),
                );

                let image_rect = egui::Rect::from_center_size(current_pos, size);
                ui.painter().image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }

            if progress >= 1.0 {
                completed_animations.push(anim.path.clone());
            }

            progress < 1.0
        });

        // Handle completed animations
        for completed_path in completed_animations {
            if let Some(idx) = self
                .pending_moves
                .iter()
                .position(|pm| pm.from == completed_path)
            {
                let pending_move = self.pending_moves.remove(idx);
                // Now move the texture
                if let Some(texture) = self.textures.remove(&pending_move.from) {
                    self.textures.insert(pending_move.to, texture);
                }
            }
        }
    }
}

impl eframe::App for ImageSorter {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Logo in top right
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.heading("LeftRight");
                });
            });
            ui.add_space(8.0);
        });

        // Main content
        if !self.setup_done {
            // Start loading images in background while setting up categories
            if !self.is_loading {
                self.start_background_loading(ctx);
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                // Category setup window in center
                let window_size = egui::vec2(400.0, 200.0);
                let window_pos = ui.available_rect_before_wrap().center() - (window_size / 2.0);

                egui::Window::new("Setup Categories")
                    .fixed_pos(window_pos)
                    .fixed_size(window_size)
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.heading("Enter Categories");
                            ui.add_space(10.0);
                            ui.label("Separate with commas (1-4 categories)");
                            ui.add_space(10.0);
                            let response = ui.text_edit_singleline(&mut self.input_categories);

                            if response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                self.categories = self
                                    .input_categories
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .take(4)
                                    .collect();
                                if !self.categories.is_empty() {
                                    self.setup_categories(ctx);
                                    self.setup_done = true;
                                }
                            }
                        });
                    });

                // Shortcuts help box on the right
                egui::Window::new("Shortcuts")
                    .fixed_pos([ui.available_rect_before_wrap().right() - 200.0, 50.0])
                    .fixed_size([180.0, 200.0])
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.vertical(|ui| {
                            ui.label("← Left category");
                            ui.label("→ Right category");
                            ui.label("↑ Up category");
                            ui.label("↓ Down category");
                            ui.add_space(5.0);
                            ui.label("Ctrl+Z Undo last move");
                        });
                    });

                // Loading progress in bottom right
                if self.is_loading {
                    let progress_width = 200.0;
                    let progress_height = 40.0;
                    let progress_pos = ui.available_rect_before_wrap().right_bottom()
                        - egui::vec2(progress_width + 20.0, progress_height + 20.0);

                    egui::Window::new("Loading")
                        .fixed_pos(progress_pos)
                        .fixed_size([progress_width, progress_height])
                        .title_bar(false)
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.add(
                                egui::ProgressBar::new(self.loading_progress)
                                    .show_percentage()
                                    .animate(true),
                            );
                            ui.label(format!(
                                "Loading images... ({}/{})",
                                self.textures.len(),
                                self.total_images_to_load
                            ));
                        });
                }
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                self.update_ui(ui, ctx);
            });
        }

        if !self.animations.is_empty() {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        vsync: true,
        multisampling: 4,
        ..Default::default()
    };

    eframe::run_native(
        "Image Sorter",
        options,
        Box::new(|_cc| Box::new(ImageSorter::new())),
    )
}
