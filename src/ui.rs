use crate::{loader::{load_snapshots, load_item_history}, stats::build_stats, flips::analyze};
use eframe::egui;
use egui::{
    Color32, Context, FontFamily, FontId, Margin, RichText, Visuals, Stroke, Vec2
};
use egui_extras::{Column, TableBuilder};
use egui_plot::{HoverPosition, Line, Plot, PlotPoint};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};

const DB_PATH: &str = "rs3_market.db";
const MISSING_DB_MSG: &str = "rs3_market.db not found — run decompress_db.ps1 or backfill_history.py";
const FAVORITES_PATH: &str = "favorites.json";

pub fn set_custom_style(ctx: &Context) {
    // RS3 Grand Exchange dark gold UI theme
    let mut visuals = Visuals::dark();

    // RS3 GE color palette
    visuals.panel_fill = Color32::from_rgb(20, 16, 10);          // Deep brown panel
    visuals.window_fill = Color32::from_rgb(28, 23, 16);         // RS3 window background
    visuals.extreme_bg_color = Color32::from_rgb(40, 32, 22);    // hover highlight
    visuals.faint_bg_color = Color32::from_rgb(35, 28, 18);      // subtle background
    
    // Widget colors with RS3 gold accents
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(45, 38, 28);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(80, 65, 40));
    
    visuals.widgets.hovered.bg_fill  = Color32::from_rgb(70, 55, 38);
    visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, Color32::from_rgb(200, 160, 80));
    
    visuals.widgets.active.bg_fill   = Color32::from_rgb(90, 70, 45);
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, Color32::from_rgb(255, 200, 100));

    // Selection colors
    visuals.selection.bg_fill = Color32::from_rgb(100, 80, 50);
    visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(255, 210, 100));

    // Apply to both dark and light styles so the theme is the same whatever the OS preference
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();

        // RS3 fonts + spacing
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.window_margin = Margin::same(12);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.indent = 16.0;

        style.text_styles.insert(
            egui::TextStyle::Body,
            FontId::new(15.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(22.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            FontId::new(14.0, FontFamily::Monospace),
        );
    });
}

#[derive(Clone)]
struct Row {
    item_id: i32,
    name: String,
    score: i32,
    tier: String,
    buy: f64,
    sell: i32,
    ge_limit: Option<i32>,  // None = unknown, qty not capped
    unit_profit: f64,  // Net profit per item after tax
    qty: i32,
    profit: f64,  // Total profit for qty items
    roi: f64,
    notes: String,
    trend: f64,  // Price trend indicator (% per sample)
    total_cost: f64,  // Total cost of buying qty items
    avg_volume: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortBy {
    Score,
    Profit,
    Roi,
    Name,
    Buy,
    Sell,
}

impl SortBy {
    fn label(self) -> &'static str {
        match self {
            SortBy::Score => "Score",
            SortBy::Profit => "Profit",
            SortBy::Roi => "ROI",
            SortBy::Name => "Name",
            SortBy::Buy => "Buy",
            SortBy::Sell => "Sell",
        }
    }
}

type LoadResult = Result<(Vec<Row>, usize), String>;

// Runs on a background thread: DB query + stats + flip analysis.
// Quantities are left at 0; they depend on the budget and are filled in on the UI thread.
fn compute_rows(db_path: &str) -> LoadResult {
    if !std::path::Path::new(db_path).exists() {
        return Err(MISSING_DB_MSG.to_string());
    }

    let (snaps, skipped) = load_snapshots(db_path).map_err(|e| {
        // An empty file (e.g. created by an older build) opens fine but has no tables
        if e.to_string().contains("no such table") {
            MISSING_DB_MSG.to_string()
        } else {
            format!("Failed to load {}: {}", db_path, e)
        }
    })?;

    let stats = build_stats(&snaps);

    let mut rows = vec![];

    for s in stats {
        if s.data_points < 1 { continue; }

        let f = analyze(&s);

        rows.push(Row {
            item_id: s.item_id,
            name: s.name.clone(),
            score: f.score,
            tier: f.tier.clone(),
            buy: f.buy as f64,
            sell: f.sell,
            ge_limit: s.ge_limit,
            unit_profit: f.profit,
            qty: 0,
            profit: 0.0,
            roi: f.roi,
            notes: f.notes.clone(),
            trend: s.price_trend,
            total_cost: 0.0,
            avg_volume: f.avg_volume,
        });
    }

    Ok((rows, skipped))
}

// Single source for the trend thresholds (relative trend, % per sample)
fn trend_indicator(trend: f64) -> (&'static str, &'static str, Color32) {
    if trend > 1.0 {
        ("↑↑", "Rising++", Color32::from_rgb(100, 255, 100))
    } else if trend > 0.2 {
        ("↑", "Rising+", Color32::from_rgb(150, 255, 150))
    } else if trend < -1.0 {
        ("↓↓", "Falling--", Color32::from_rgb(255, 100, 100))
    } else if trend < -0.2 {
        ("↓", "Falling-", Color32::from_rgb(255, 150, 150))
    } else {
        ("→", "Stable", Color32::from_rgb(200, 200, 200))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortOrder {
    Ascending,
    Descending,
}

pub struct RS3App {
    loaded: bool,
    items: Vec<Row>,
    filtered_items: Vec<Row>,
    search: String,
    budget: f64,
    show_bad: bool,
    
    // Sorting
    sort_by: SortBy,
    sort_order: SortOrder,
    
    // Filters
    min_profit: f64,
    min_roi: f64,
    selected_tier: Option<String>,
    show_favorites_only: bool,
    
    // UI state
    selected_item: Option<i32>,  // item_id, stable across re-sort/filter

    favorites: HashMap<String, bool>,  // Keyed by name for compatibility with existing files
    favorites_writable: bool,  // false if an existing favorites file could not be read or backed up

    selected_item_history: Vec<(String, f64)>,
    history_points: Vec<PlotPoint>,  // Cached plot points for selected_item_history
    history_y_range: (f64, f64),
    plot_needs_reset: bool,
    graph_height: f32,
    target_graph_height: f32,

    // Background load
    load_rx: Option<Receiver<LoadResult>>,
    status: String,
    status_is_error: bool,
}

impl RS3App {
    pub fn new() -> Self {
        let (favorites, favorites_writable, favorites_status) = Self::load_favorites();
        Self {
            loaded: false,
            items: vec![],
            filtered_items: vec![],
            search: "".into(),
            budget: 50_000_000.0,
            show_bad: false,
            
            sort_by: SortBy::Score,
            sort_order: SortOrder::Descending,
            
            min_profit: 0.0,
            min_roi: 0.0,
            selected_tier: None,
            show_favorites_only: false,
            
            selected_item: None,

            favorites,
            favorites_writable,

            selected_item_history: vec![],
            history_points: vec![],
            history_y_range: (0.0, 0.0),
            plot_needs_reset: false,
            graph_height: 0.0,
            target_graph_height: 0.0,

            load_rx: None,
            status_is_error: favorites_status.is_some(),
            status: favorites_status.unwrap_or_default(),
        }
    }

    /// Returns (favorites, writable, status message).
    /// A corrupt file is moved to favorites.json.bak instead of being silently overwritten.
    fn load_favorites() -> (HashMap<String, bool>, bool, Option<String>) {
        use std::fs;
        let data = match fs::read_to_string(FAVORITES_PATH) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (HashMap::new(), true, None),
            Err(e) => {
                return (HashMap::new(), false, Some(format!(
                    "Could not read {}: {} — favorites will not be saved this session", FAVORITES_PATH, e)));
            }
        };
        match serde_json::from_str(&data) {
            Ok(map) => (map, true, None),
            Err(e) => {
                let bak = format!("{}.bak", FAVORITES_PATH);
                match fs::rename(FAVORITES_PATH, &bak) {
                    Ok(()) => (HashMap::new(), true, Some(format!(
                        "{} was corrupt ({}); moved it to {} and started with no favorites", FAVORITES_PATH, e, bak))),
                    Err(re) => (HashMap::new(), false, Some(format!(
                        "{} is corrupt ({}) and could not be backed up ({}) — favorites will not be saved this session",
                        FAVORITES_PATH, e, re))),
                }
            }
        }
    }

    // Write to a temp file then rename, so a crash mid-write never leaves a truncated file
    fn save_favorites(&mut self) {
        use std::fs;
        if !self.favorites_writable {
            self.set_status(format!("Favorites not saved: existing {} could not be read or backed up", FAVORITES_PATH), true);
            return;
        }
        let tmp = format!("{}.tmp", FAVORITES_PATH);
        let result = serde_json::to_string(&self.favorites)
            .map_err(|e| e.to_string())
            .and_then(|json| fs::write(&tmp, json).map_err(|e| e.to_string()))
            .and_then(|()| fs::rename(&tmp, FAVORITES_PATH).map_err(|e| e.to_string()));
        if let Err(e) = result {
            self.set_status(format!("Failed to save {}: {}", FAVORITES_PATH, e), true);
        }
    }

    fn set_status(&mut self, msg: String, is_error: bool) {
        self.status = msg;
        self.status_is_error = is_error;
    }

    fn start_load(&mut self, ctx: &Context) {
        if self.load_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(compute_rows(DB_PATH));
            ctx.request_repaint();
        });
        self.load_rx = Some(rx);
        self.set_status("Scanning market...".to_string(), false);
    }

    fn poll_load(&mut self) {
        let Some(rx) = &self.load_rx else { return };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Market scan failed unexpectedly".to_string()),
        };
        self.load_rx = None;
        match result {
            Ok((rows, skipped)) => {
                self.items = rows;
                self.loaded = true;
                // Drop the selection if the item disappeared from the new data
                if let Some(id) = self.selected_item
                    && !self.items.iter().any(|r| r.item_id == id)
                {
                    self.clear_selection();
                }
                self.recompute_quantities();
                let mut msg = format!("Loaded {} items", self.items.len());
                if skipped > 0 {
                    msg.push_str(&format!(" ({} unreadable rows skipped)", skipped));
                }
                self.set_status(msg, skipped > 0);
            }
            Err(e) => self.set_status(e, true),
        }
    }

    // Budget only affects quantity, so recompute from cached results without touching the DB
    fn recompute_quantities(&mut self) {
        for r in &mut self.items {
            let qty = if r.buy > 0.0 {
                let affordable = (self.budget / r.buy) as i32;
                match r.ge_limit {
                    Some(limit) => affordable.min(limit),
                    None => affordable,
                }
            } else {
                0
            };
            r.qty = qty;
            r.profit = (r.unit_profit * qty as f64).round();
            r.total_cost = r.buy * qty as f64;
        }
        self.apply_filters();
    }

    fn select_item(&mut self, item_id: i32) {
        self.selected_item = Some(item_id);
        self.target_graph_height = 300.0;
        self.plot_needs_reset = true;
        // Always reload history when selecting an item
        self.selected_item_history = load_item_history(DB_PATH, item_id).unwrap_or_else(|e| {
            eprintln!("load_item_history({}): {}", item_id, e);
            vec![]
        });
        self.history_points = self.selected_item_history
            .iter()
            .enumerate()
            .map(|(i, (_, price))| PlotPoint::new(i as f64, *price))
            .collect();
        let min_price = self.history_points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_price = self.history_points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        self.history_y_range = (min_price, max_price);
    }

    fn clear_selection(&mut self) {
        self.selected_item = None;
        self.target_graph_height = 0.0;
        self.selected_item_history.clear();
        self.history_points.clear();
    }

    fn apply_filters(&mut self) {
        let mut filtered = self.items.clone();

        filtered.retain(|r| r.qty > 0);
        
        filtered.retain(|r| {
            let has_volume = r.avg_volume >= 500.0;
            let reasonable_roi = r.roi <= 150.0;
            let reasonable_price = r.buy >= 100.0;
            has_volume && reasonable_roi && reasonable_price
        });
        
        if self.show_favorites_only {
            filtered.retain(|r| self.favorites.get(&r.name).copied().unwrap_or(false));
        }

        if !self.search.is_empty() {
            let search_lower = self.search.to_lowercase();
            filtered.retain(|r| r.name.to_lowercase().contains(&search_lower));
        }

        if let Some(ref tier) = self.selected_tier {
            filtered.retain(|r| &r.tier == tier);
        }

        if self.min_profit > 0.0 {
            filtered.retain(|r| r.profit >= self.min_profit);
        }

        if self.min_roi > 0.0 {
            filtered.retain(|r| r.roi >= self.min_roi);
        }

        if !self.show_bad {
            filtered.retain(|r| r.score >= 0);
        }

        match self.sort_by {
            SortBy::Score => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.score.cmp(&a.score)
                } else {
                    a.score.cmp(&b.score)
                }
            }),
            SortBy::Profit => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.profit.total_cmp(&a.profit)
                } else {
                    a.profit.total_cmp(&b.profit)
                }
            }),
            SortBy::Roi => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.roi.total_cmp(&a.roi)
                } else {
                    a.roi.total_cmp(&b.roi)
                }
            }),
            SortBy::Name => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.name.cmp(&a.name)
                } else {
                    a.name.cmp(&b.name)
                }
            }),
            SortBy::Buy => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.buy.total_cmp(&a.buy)
                } else {
                    a.buy.total_cmp(&b.buy)
                }
            }),
            SortBy::Sell => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.sell.cmp(&a.sell)
                } else {
                    a.sell.cmp(&b.sell)
                }
            }),
        }

        self.filtered_items = filtered;
    }

    fn tier_color(&self, t: &str) -> Color32 {
        match t {
            "DIAMOND" => Color32::from_rgb(0, 255, 255),
            "GOLD"    => Color32::from_rgb(255, 200, 50),
            "GREEN"   => Color32::from_rgb(50, 255, 50),
            "CRASH"   => Color32::RED,
            _         => Color32::LIGHT_GRAY,
        }
    }
}

impl eframe::App for RS3App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_load();
        let loading = self.load_rx.is_some();

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            
            // Title bar with RS3 styling
            ui.horizontal(|ui| {
                ui.heading(RichText::new("⚔ RuneScape 3 Grand Exchange Analyzer")
                    .color(Color32::from_rgb(255, 210, 100))
                    .strong()
                    .size(24.0)
                );
                

            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                // Scan button - prominent RS3 style
                let scan_clicked = ui.add_enabled_ui(!loading, |ui| {
                    ui.add_sized(
                        Vec2::new(100.0, 32.0),
                        egui::Button::new(RichText::new("🔍 Scan Market")
                            .color(Color32::from_rgb(255, 220, 100))
                            .strong())
                    ).clicked()
                }).inner;
                if scan_clicked {
                    self.start_load(ui.ctx());
                }
                if loading {
                    ui.spinner();
                }

                ui.separator();

                // Budget control
                ui.label(RichText::new("💰 Budget:")
                    .color(Color32::from_rgb(200, 180, 100)));
                
                let mut b = self.budget / 1_000_000.0;
                if ui.add(egui::DragValue::new(&mut b)
                    .suffix(" M")
                    .speed(1.0))
                    .changed() 
                {
                    self.budget = (b * 1_000_000.0).max(100_000.0);
                    if self.loaded {
                        self.recompute_quantities();
                    }
                }

                ui.separator();

                // Search box
                ui.label(RichText::new("🔎").color(Color32::from_rgb(200, 180, 100)));
                let search_response = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search items...")
                        .desired_width(200.0)
                );
                if search_response.changed() && self.loaded {
                    self.apply_filters();
                }

                if !self.status.is_empty() {
                    ui.separator();
                    let color = if self.status_is_error {
                        Color32::from_rgb(255, 120, 100)
                    } else {
                        Color32::from_rgb(180, 160, 120)
                    };
                    ui.label(RichText::new(&self.status).color(color));
                }

            });

            ui.add_space(2.0);
        });

        if self.loaded {
            egui::Panel::right("filters")
                .min_size(250.0)
                .max_size(350.0)
                .show(ui, |ui| {
                    ui.heading(RichText::new("⚡ Filters & Settings")
                        .color(Color32::from_rgb(255, 210, 100)));
                    
                    ui.separator();
                    
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // Tier filter
                        ui.label(RichText::new("🏆 Tier Filter").strong());
                        ui.horizontal_wrapped(|ui| {
                            let tiers = [
                                ("DIAMOND", "💎 Diamond"),
                                ("GOLD", "⭐ Gold"),
                                ("GREEN", "✅ Good"),
                                ("NORMAL", "⚪ Normal"),
                                ("CRASH", "📉 Crash"),
                            ];
                            for (tier, label) in tiers {
                                let is_selected = self.selected_tier.as_deref() == Some(tier);
                                let col = self.tier_color(tier);
                                
                                if ui.selectable_label(
                                    is_selected,
                                    RichText::new(label).color(col)
                                ).clicked() {
                                    self.selected_tier = if is_selected {
                                        None
                                    } else {
                                        Some(tier.to_string())
                                    };
                                    self.apply_filters();
                                }
                            }
                        });
                        
                        if self.selected_tier.is_some() && ui.button("Clear Tier Filter").clicked() {
                            self.selected_tier = None;
                            self.apply_filters();
                        }
                        
                        ui.add_space(10.0);
                        ui.separator();
                        
                        // Profit filter
                        ui.label(RichText::new("💎 Min Profit").strong());
                        let mut min_profit_m = self.min_profit / 1_000_000.0;
                        if ui.add(egui::Slider::new(&mut min_profit_m, 0.0..=10.0)
                            .suffix(" M")
                            .step_by(0.1))
                            .changed() 
                        {
                            self.min_profit = min_profit_m * 1_000_000.0;
                            self.apply_filters();
                        }
                        
                        ui.add_space(10.0);
                        
                        // ROI filter
                        ui.label(RichText::new("📈 Min ROI").strong());
                        if ui.add(egui::Slider::new(&mut self.min_roi, 0.0..=100.0)
                            .suffix("%")
                            .step_by(1.0))
                            .changed() 
                        {
                            self.apply_filters();
                        }
                        
                        ui.add_space(10.0);
                        ui.separator();
                        
                        // Show bad items toggle
                        if ui.checkbox(&mut self.show_bad, "Show Negative Score Items")
                            .changed() 
                        {
                            self.apply_filters();
                        }
                        
                        ui.add_space(10.0);
                        
                        // Favorites filter
                        if ui.checkbox(&mut self.show_favorites_only, "⭐ Show Favorites Only")
                            .changed() 
                        {
                            self.apply_filters();
                        }
                        
                        ui.add_space(10.0);
                        ui.separator();
                        
                        // Sorting options
                        ui.label(RichText::new("📊 Sort By").strong());
                        
                        egui::ComboBox::from_id_salt("sort_by")
                            .selected_text(self.sort_by.label())
                            .show_ui(ui, |ui| {
                                let sorts = [
                                    SortBy::Score,
                                    SortBy::Profit,
                                    SortBy::Roi,
                                    SortBy::Name,
                                    SortBy::Buy,
                                    SortBy::Sell
                                ];
                                for sort in sorts {
                                    if ui.selectable_value(&mut self.sort_by, sort, sort.label()).clicked() {
                                        self.apply_filters();
                                    }
                                }
                            });
                        
                        ui.horizontal(|ui| {
                            if ui.selectable_value(&mut self.sort_order, SortOrder::Descending, "⬇ Desc")
                                .clicked() 
                            {
                                self.apply_filters();
                            }
                            if ui.selectable_value(&mut self.sort_order, SortOrder::Ascending, "⬆ Asc")
                                .clicked() 
                            {
                                self.apply_filters();
                            }
                        });
                        
                        ui.add_space(10.0);
                        ui.separator();
                        
                        // Reset filters
                        if ui.button(RichText::new("🔄 Reset All Filters")
                            .color(Color32::from_rgb(255, 150, 150)))
                            .clicked() 
                        {
                            self.min_profit = 0.0;
                            self.min_roi = 0.0;
                            self.selected_tier = None;
                            self.show_bad = false;
                            self.sort_by = SortBy::Score;
                            self.sort_order = SortOrder::Descending;
                            self.apply_filters();
                        }
                    });
                });
        }

        // Frame-rate independent ease: ~0.2 of the remaining distance per frame at 60 fps
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.graph_height += (self.target_graph_height - self.graph_height) * (1.0 - (-13.4 * dt).exp());
        if (self.target_graph_height - self.graph_height).abs() < 0.5 {
            self.graph_height = self.target_graph_height;
        }

        // Look the selection up in all items (not just filtered ones) so the panel and its ✖
        // stay available when the selected item is filtered out
        let selected_row = self.selected_item
            .and_then(|id| self.items.iter().find(|r| r.item_id == id));
        let mut close_clicked = false;
        let mut plot_rendered = false;

        if self.graph_height > 1.0 {
            egui::Panel::bottom("graph_panel")
                .exact_size(self.graph_height)
                .resizable(false)
                .show(ui, |ui| {
                    if let Some(r) = selected_row {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(35, 28, 18))
                            .stroke(Stroke::new(2.0, Color32::from_rgb(100, 80, 50)))
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("⎘ Price History:")
                                        .color(Color32::from_rgb(255, 210, 100))
                                        .strong()
                                        .size(16.0));

                                    ui.label(RichText::new(&r.name)
                                        .color(Color32::from_rgb(255, 220, 150))
                                        .size(16.0));

                                    ui.separator();

                                    ui.label(RichText::new(&r.notes)
                                        .color(Color32::from_rgb(180, 160, 120))
                                        .italics());

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(RichText::new("✖").size(16.0)).clicked() {
                                            close_clicked = true;
                                        }
                                    });
                                });

                                if !self.history_points.is_empty() {
                                    ui.add_space(5.0);

                                    let line = Line::new("Price", &self.history_points[..])
                                        .color(Color32::from_rgb(100, 200, 255))
                                        .width(2.0);

                                    // Calculate Y-axis range to handle negative values
                                    let (min_price, max_price) = self.history_y_range;
                                    let padding = (max_price - min_price) * 0.1;

                                    let history = &self.selected_item_history;
                                    let mut plot = Plot::new("price_history")
                                        .height(self.graph_height - 80.0)
                                        .show_axes(true)
                                        .show_grid(true)
                                        .allow_zoom(true)
                                        .allow_scroll(true)
                                        .allow_drag(true)
                                        .include_y(min_price - padding)
                                        .include_y(max_price + padding)
                                        .x_axis_formatter(move |mark, _range| {
                                            match history_date(history, mark.value) {
                                                Some(date) if date.len() >= 10 => date[5..10].to_string(),
                                                Some(date) => date.clone(),
                                                None => String::new(),
                                            }
                                        })
                                        .label_formatter(move |pos| {
                                            let (date, value) = match pos {
                                                HoverPosition::NearDataPoint { position, index, .. } => {
                                                    (history.get(*index).map(|(d, _)| d), position)
                                                }
                                                HoverPosition::Elsewhere { position } => {
                                                    (history_date(history, position.x), position)
                                                }
                                            };
                                            Some(if let Some(date) = date {
                                                format!("{}\nPrice: {}", date, format_gp(value.y))
                                            } else {
                                                format!("Price: {}", format_gp(value.y))
                                            })
                                        });
                                    // Reset the view only when the selection changes, so zoom/pan sticks
                                    if self.plot_needs_reset {
                                        plot = plot.reset();
                                    }
                                    plot.show(ui, |plot_ui| {
                                        plot_ui.line(line);
                                    });
                                    plot_rendered = true;
                                } else {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(RichText::new("No price history")
                                            .color(Color32::from_rgb(180, 160, 120))
                                            .italics()
                                            .size(18.0));
                                    });
                                }
                            });
                    }
                });
        }

        if plot_rendered {
            self.plot_needs_reset = false;
        }
        if close_clicked || (self.selected_item.is_some() && selected_row.is_none()) {
            self.clear_selection();
        }

        egui::CentralPanel::default().show(ui, |ui| {

            if !self.loaded {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.label(RichText::new("⚔")
                            .size(80.0)
                            .color(Color32::from_rgb(255, 210, 100)));
                        ui.add_space(20.0);
                        ui.label(RichText::new("Welcome to the Grand Exchange Analyzer")
                            .size(24.0)
                            .color(Color32::from_rgb(200, 180, 140)));
                        ui.add_space(10.0);
                        if loading {
                            ui.spinner();
                            ui.label(RichText::new("Scanning market...")
                                .color(Color32::from_rgb(180, 160, 120)));
                        } else if self.status_is_error && !self.status.is_empty() {
                            ui.label(RichText::new(&self.status)
                                .color(Color32::from_rgb(255, 120, 100)));
                        } else {
                            ui.label(RichText::new("Click 'Scan Market' to begin analyzing flips")
                                .color(Color32::from_rgb(180, 160, 120)));
                        }
                    });
                });
                return;
            }

            if self.filtered_items.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("🔍")
                            .size(60.0)
                            .color(Color32::from_rgb(150, 130, 100)));
                        ui.add_space(10.0);
                        ui.label(RichText::new("No items match your filters")
                            .size(20.0)
                            .color(Color32::from_rgb(180, 160, 120)));
                        ui.add_space(5.0);
                        ui.label(RichText::new("Try adjusting your search or filter settings")
                            .color(Color32::from_rgb(150, 130, 100)));
                    });
                });
                return;
            }

            ui.style_mut().visuals.extreme_bg_color = Color32::from_rgb(45, 38, 28);

            use std::cell::RefCell;
            use std::rc::Rc;
            let favorite_toggles = Rc::new(RefCell::new(Vec::new()));
            let toggles_clone = favorite_toggles.clone();
            let mut clicked_item: Option<i32> = None;

            TableBuilder::new(ui)
                .striped(true)
                .vscroll(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(40.0))   // Favorite
                .column(Column::remainder().at_least(180.0).clip(true))  // Item - takes remaining space
                .column(Column::exact(80.0))   // Score
                .column(Column::exact(120.0))  // Tier + Trend
                .column(Column::exact(130.0))  // Risk Warning
                .column(Column::exact(110.0))  // Buy
                .column(Column::exact(110.0))  // Sell
                .column(Column::exact(70.0))   // Qty                
                .column(Column::exact(120.0))  // Avg Volume                
                .column(Column::exact(120.0))  // Total Cost
                .column(Column::exact(120.0))  // Profit
                .column(Column::exact(90.0))   // ROI
                .column(Column::exact(50.0))   // Copy button
                .header(32.0, |mut header| {
                    header.col(|ui| { 
                        ui.heading(RichText::new("★").color(Color32::from_rgb(255, 200, 50)).size(20.0)); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Item Name")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Score")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Tier/Trend")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("⚠ Risk")
                            .color(Color32::from_rgb(255, 180, 100))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Buy Price")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Sell Price")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Qty")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Avg Vol/Day")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Total Cost")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("Profit")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("ROI %")
                            .color(Color32::from_rgb(200, 180, 140))); 
                    });
                    header.col(|ui| { 
                        ui.heading(RichText::new("📋").color(Color32::from_rgb(200, 180, 140)).size(18.0)); 
                    });
                })
                .body(|body| {
                    body.rows(36.0, self.filtered_items.len(), |mut row| {
                        let i = row.index();
                        let r = &self.filtered_items[i];
                        
                        let is_selected = self.selected_item == Some(r.item_id);
                        let is_favorite = self.favorites.get(&r.name).copied().unwrap_or(false);
                        let item_name = r.name.clone();

                        // Favorite button
                        row.col(|ui| {
                            let toggles = toggles_clone.clone();
                            if ui.button(RichText::new(if is_favorite { "★" } else { "☆" })
                                .color(if is_favorite { 
                                    Color32::from_rgb(255, 200, 50) 
                                } else { 
                                    Color32::GRAY 
                                }))
                                .clicked() 
                            {
                                toggles.borrow_mut().push(item_name);
                            }
                        });

                        row.col(|ui| {
                            let mut text = RichText::new(&r.name);
                            if is_selected {
                                text = text.color(Color32::from_rgb(255, 220, 100)).strong();
                            } else if is_favorite {
                                text = text.color(Color32::from_rgb(255, 200, 100));
                            }
                            if ui.selectable_label(is_selected, text).clicked() {
                                clicked_item = Some(r.item_id);
                            }
                        });

                        // Score with color coding
                        row.col(|ui| {
                            let score_color = if r.score > 100 {
                                Color32::from_rgb(100, 255, 100)
                            } else if r.score > 50 {
                                Color32::from_rgb(200, 200, 100)
                            } else if r.score > 0 {
                                Color32::from_rgb(200, 150, 100)
                            } else {
                                Color32::from_rgb(255, 100, 100)
                            };
                            ui.label(RichText::new(r.score.to_string())
                                .color(score_color)
                                .strong());
                        });

                        // Tier badge with trend indicator
                        row.col(|ui| {
                            ui.centered_and_justified(|ui| {
                                ui.horizontal(|ui| {
                                    // Tier icon with matching color from tier_color
                                    let (icon, tier_name, tier_color) = match r.tier.as_str() {
                                        "DIAMOND" => ("💎", "Diamond", Color32::from_rgb(0, 255, 255)),
                                        "GOLD" => ("⭐", "Gold", Color32::from_rgb(255, 200, 50)),
                                        "GREEN" => ("✅", "Good", Color32::from_rgb(50, 255, 50)),
                                        "CRASH" => ("📉", "Crash", Color32::RED),
                                        _ => ("⚪", "Normal", Color32::LIGHT_GRAY),
                                    };
                                    ui.label(RichText::new(icon).size(16.0).color(tier_color))
                                        .on_hover_text(tier_name);
                                    
                                    // Trend indicator with clear text label
                                    let (trend_text, _, trend_color) = trend_indicator(r.trend);
                                    ui.label(RichText::new(trend_text).color(trend_color).strong());
                                });
                            });
                        });

                        // Risk Warning
                        row.col(|ui| {
                            // Parse notes to extract risk warning
                            let (risk_text, risk_color) = if r.notes.contains("🚨VOLATILE-CRASHING") {
                                ("🚨 Crashing", Color32::from_rgb(255, 100, 100))
                            } else if r.notes.contains("📉Crashed") {
                                ("📉 Crashed", Color32::from_rgb(255, 150, 100))
                            } else if r.notes.contains("📈Spiked") {
                                ("📈 Spiked", Color32::from_rgb(255, 200, 100))
                            } else if r.notes.contains("⚠outliers") {
                                ("⚠ Outliers", Color32::from_rgb(200, 180, 100))
                            } else {
                                ("● Stable", Color32::from_rgb(100, 255, 150))
                            };
                            
                            ui.label(RichText::new(risk_text)
                                .color(risk_color)
                                .strong())
                                .on_hover_text(&r.notes);
                        });

                        // Buy price
                        row.col(|ui| {
                            ui.label(RichText::new(format_gp(r.buy))
                                .color(Color32::from_rgb(255, 150, 150)));
                        });

                        // Sell price
                        row.col(|ui| {
                            ui.label(RichText::new(format!("{:>10}", format_gp(r.sell as f64)))
                                .color(Color32::from_rgb(150, 255, 150)));
                        });

                        // Quantity
                        row.col(|ui| {
                            // "?" = GE limit unknown, qty is not capped by a limit
                            let (qty_text, limit_text) = match r.ge_limit {
                                Some(limit) => (r.qty.to_string(), format!("GE limit: {}", limit)),
                                None => (format!("{}?", r.qty), "GE limit: ? (unknown, qty not capped)".to_string()),
                            };
                            ui.label(RichText::new(qty_text)
                                .color(Color32::from_rgb(200, 200, 200)))
                                .on_hover_text(limit_text);
                        });

                        // Avg Volume/Day
                        row.col(|ui| {
                            let vol_text = if r.avg_volume >= 1_000_000.0 {
                                format!("{:.1}M", r.avg_volume / 1_000_000.0)
                            } else if r.avg_volume >= 1_000.0 {
                                format!("{:.1}K", r.avg_volume / 1_000.0)
                            } else {
                                format!("{:.0}", r.avg_volume)
                            };
                            ui.label(RichText::new(vol_text)
                                .color(Color32::from_rgb(180, 200, 255)));
                        });

                        // Total Cost
                        row.col(|ui| {
                            ui.label(RichText::new(format_gp(r.total_cost))
                                .color(Color32::from_rgb(200, 180, 255)));
                        });

                        // Profit with highlighting
                        row.col(|ui| {
                            let profit_color = if r.profit > 1_000_000.0 {
                                Color32::from_rgb(100, 255, 100)
                            } else if r.profit > 100_000.0 {
                                Color32::from_rgb(150, 255, 150)
                            } else if r.profit > 0.0 {
                                Color32::from_rgb(200, 255, 200)
                            } else {
                                Color32::from_rgb(255, 100, 100)
                            };
                            ui.label(RichText::new(format_gp(r.profit))
                                .color(profit_color)
                                .strong());
                        });

                        // ROI with color coding
                        row.col(|ui| {
                            let roi_color = if r.roi > 20.0 {
                                Color32::from_rgb(100, 255, 100)
                            } else if r.roi > 10.0 {
                                Color32::from_rgb(150, 255, 150)
                            } else if r.roi > 5.0 {
                                Color32::from_rgb(200, 255, 200)
                            } else if r.roi > 0.0 {
                                Color32::from_rgb(255, 255, 150)
                            } else {
                                Color32::from_rgb(255, 100, 100)
                            };
                            ui.label(RichText::new(format!("{:.1}%", r.roi))
                                .color(roi_color));
                        });

                        // Copy button
                        row.col(|ui| {
                            if ui.button("📋").on_hover_text("Copy item details").clicked() {
                                let (_, trend_text, _) = trend_indicator(r.trend);
                                let limit_note = if r.ge_limit.is_none() { " (GE limit ?)" } else { "" };

                                let copy_text = format!(
                                    "{}:\nScore: {}\nTier: {} {}\nBuy: {}\nSell: {}\nQty: {}{}\nAvg Vol: {}\nTotal cost: {}\nProfit: {}\nROI: {:.1}%",
                                    r.name,
                                    r.score,
                                    r.tier,
                                    trend_text,
                                    format_gp(r.buy),
                                    format_gp(r.sell as f64),
                                    r.qty,
                                    limit_note,
                                    r.avg_volume,
                                    format_gp(r.total_cost),
                                    format_gp(r.profit),
                                    r.roi
                                );
                                
                                ui.ctx().copy_text(copy_text);
                            }
                        });
                    });
                });
            
            // Process favorite toggles
            let toggles = favorite_toggles.borrow();
            for item_name in toggles.iter() {
                let current = self.favorites.get(item_name).copied().unwrap_or(false);
                self.favorites.insert(item_name.clone(), !current);
            }
            if !toggles.is_empty() {
                self.save_favorites();
                self.apply_filters();
            }

            if let Some(id) = clicked_item {
                if self.selected_item == Some(id) {
                    self.clear_selection();
                } else {
                    self.select_item(id);
                }
            }
        });

        // Only keep repainting while something is changing on its own
        // (checked at end of frame so a selection made this frame starts the animation)
        let animating = self.graph_height != self.target_graph_height;
        if animating || self.load_rx.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

// Date label for an integer x within the history; None for fractional or out-of-range x
fn history_date(history: &[(String, f64)], x: f64) -> Option<&String> {
    let idx = x.round();
    if idx >= 0.0 && (x - idx).abs() < 1e-6 {
        history.get(idx as usize).map(|(date, _)| date)
    } else {
        None
    }
}

pub(crate) fn format_gp(value: f64) -> String {
    if value < 0.0 {
        let s = format_gp(-value);
        return if s == "0" { s } else { format!("-{}", s) };
    }
    if value >= 1_000_000_000.0 {
        format!("{:.2}B", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{:.0}", value)
    }
}
