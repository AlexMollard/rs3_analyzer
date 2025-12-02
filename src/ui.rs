use crate::{loader::load_snapshots, stats::build_stats, flips::analyze};
use eframe::egui;
use egui::{
    Color32, Context, FontFamily, FontId, Margin, RichText, Visuals, Stroke, Vec2
};
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;

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

    ctx.set_visuals(visuals);

    // RS3 fonts + spacing
    let mut style = (*ctx.style()).clone();

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

    ctx.set_style(style);
}

#[derive(Clone)]
struct Row {
    name: String,
    score: i32,
    tier: String,
    buy: f64,
    sell: i32,
    qty: i32,
    profit: f64,
    roi: f64,
    notes: String,
    trend: f64,  // Price trend indicator
    total_cost: f64,  // Total cost of buying qty items
    avg_volume: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortBy {
    Score,
    Profit,
    ROI,
    Name,
    Buy,
    Sell,
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
    selected_row: Option<usize>,
    
    // Favorites
    favorites: HashMap<String, bool>,
}

impl RS3App {
    pub fn new() -> Self {
        let favorites = Self::load_favorites();
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
            
            selected_row: None,
            
            favorites,
        }
    }
    
    fn load_favorites() -> HashMap<String, bool> {
        use std::fs;
        if let Ok(data) = fs::read_to_string("favorites.json") {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            HashMap::new()
        }
    }
    
    fn save_favorites(&self) {
        use std::fs;
        if let Ok(json) = serde_json::to_string(&self.favorites) {
            let _ = fs::write("favorites.json", json);
        }
    }

    fn load_data(&mut self) {
        let tax = 0.02;

        let result = load_snapshots("rs3_market.db");

        let snaps = match result {
            Ok(v) => v,
            Err(_) => return,
        };

        let stats = build_stats(&snaps);

        let mut rows = vec![];

        for s in stats {
            if s.data_points < 1 { continue; }

            let f = analyze(&s, tax);

            let qty = if f.buy > 0 {
                ((self.budget / f.buy as f64) as i32).min(s.ge_limit)
            } else {
                0
            };
            let total_profit = f.profit as f64 * qty as f64;
            let total_cost = f.buy as f64 * qty as f64;

            rows.push(Row {
                name: s.name.clone(),
                score: f.score,
                tier: f.tier.clone(),
                buy: f.buy as f64,
                sell: f.sell,
                qty,
                profit: total_profit,
                roi: f.roi,
                notes: f.notes.clone(),
                trend: s.price_trend,
                total_cost,
                avg_volume: f.avg_volume,
            });
        }

        self.items = rows;
        self.apply_filters();
        self.loaded = true;
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
            filtered.retain(|r| r.score > 0);
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
                    b.profit.partial_cmp(&a.profit).unwrap()
                } else {
                    a.profit.partial_cmp(&b.profit).unwrap()
                }
            }),
            SortBy::ROI => filtered.sort_by(|a, b| {
                if self.sort_order == SortOrder::Descending {
                    b.roi.partial_cmp(&a.roi).unwrap()
                } else {
                    a.roi.partial_cmp(&b.roi).unwrap()
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
                    b.buy.partial_cmp(&a.buy).unwrap()
                } else {
                    a.buy.partial_cmp(&b.buy).unwrap()
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

    fn tier_badge<'a>(&self, ui: &mut egui::Ui, tier: &str) {
        let (icon, text, color) = match tier {
            "DIAMOND" => ("◆", "Diamond", Color32::from_rgb(100, 200, 255)),
            "GOLD" => ("★", "Gold", Color32::from_rgb(255, 215, 0)),
            "GREEN" => ("✓", "Good", Color32::from_rgb(100, 255, 150)),
            "CRASH" => ("▼", "Crash", Color32::from_rgb(255, 100, 100)),
            _ => ("●", "Normal", Color32::from_rgb(180, 180, 180)),
        };
        
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).size(18.0).color(color));
            ui.label(RichText::new(text).color(color).small());
        });
    }
}

impl eframe::App for RS3App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
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
                if ui.add_sized(
                    Vec2::new(100.0, 32.0),
                    egui::Button::new(RichText::new("🔍 Scan Market")
                        .color(Color32::from_rgb(255, 220, 100))
                        .strong())
                ).clicked() {
                    self.load_data();
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
                        self.load_data();
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
                if search_response.changed() {
                    if self.loaded {
                        self.apply_filters();
                    }
                }

            });

            ui.add_space(2.0);
        });

        if self.loaded {
            egui::SidePanel::right("filters")
                .min_width(250.0)
                .max_width(350.0)
                .show(ctx, |ui| {
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
                        
                        if self.selected_tier.is_some() {
                            if ui.button("Clear Tier Filter").clicked() {
                                self.selected_tier = None;
                                self.apply_filters();
                            }
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
                            .selected_text(format!("{:?}", self.sort_by))
                            .show_ui(ui, |ui| {
                                let sorts = [
                                    SortBy::Score, 
                                    SortBy::Profit, 
                                    SortBy::ROI, 
                                    SortBy::Name,
                                    SortBy::Buy,
                                    SortBy::Sell
                                ];
                                for sort in sorts {
                                    if ui.selectable_value(&mut self.sort_by, sort, format!("{:?}", sort)).clicked() {
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

        egui::CentralPanel::default().show(ctx, |ui| {

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
                        ui.label(RichText::new("Click 'Scan Market' to begin analyzing flips")
                            .color(Color32::from_rgb(180, 160, 120)));
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

            TableBuilder::new(ui)
                .striped(true)
                .vscroll(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(40.0))   // Favorite
                .column(Column::remainder().at_least(180.0).clip(true))  // Item - takes remaining space
                .column(Column::exact(80.0))   // Score
                .column(Column::exact(120.0))  // Tier + Trend
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
                        
                        let is_selected = self.selected_row == Some(i);
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

                        // Item name
                        row.col(|ui| {
                            let mut text = RichText::new(&r.name);
                            if is_selected {
                                text = text.color(Color32::from_rgb(255, 220, 100)).strong();
                            } else if is_favorite {
                                text = text.color(Color32::from_rgb(255, 200, 100));
                            }
                            if ui.selectable_label(is_selected, text).clicked() {
                                self.selected_row = if is_selected { None } else { Some(i) };
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
                                    let (trend_text, trend_color) = if r.trend > 5.0 {
                                        ("↑↑", Color32::from_rgb(100, 255, 100))
                                    } else if r.trend > 1.0 {
                                        ("↑", Color32::from_rgb(150, 255, 150))
                                    } else if r.trend < -5.0 {
                                        ("↓↓", Color32::from_rgb(255, 100, 100))
                                    } else if r.trend < -1.0 {
                                        ("↓", Color32::from_rgb(255, 150, 150))
                                    } else {
                                        ("→", Color32::from_rgb(200, 200, 200))
                                    };
                                    ui.label(RichText::new(trend_text).color(trend_color).strong());
                                });
                            });
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
                            ui.label(RichText::new(r.qty.to_string())
                                .color(Color32::from_rgb(200, 200, 200)));
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
                                let (trend_text, _) = if r.trend > 5.0 {
                                    ("Rising++", Color32::from_rgb(100, 255, 100))
                                } else if r.trend > 1.0 {
                                    ("Rising+", Color32::from_rgb(150, 255, 150))
                                } else if r.trend < -5.0 {
                                    ("Falling--", Color32::from_rgb(255, 100, 100))
                                } else if r.trend < -1.0 {
                                    ("Falling-", Color32::from_rgb(255, 150, 150))
                                } else {
                                    ("Stable", Color32::from_rgb(200, 200, 200))
                                };
                                
                                let copy_text = format!(
                                    "{}:\nScore: {}\nTier: {} {}\nBuy: {}\nSell: {}\nQty: {}\nAvg Vol: {}\nTotal cost: {}\nProfit: {}\nROI: {:.1}%",
                                    r.name,
                                    r.score,
                                    r.tier,
                                    trend_text,
                                    format_gp(r.buy),
                                    format_gp(r.sell as f64),
                                    r.qty,
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
            }

            if let Some(idx) = self.selected_row {
                if let Some(r) = self.filtered_items.get(idx) {
                    ui.add_space(10.0);
                    ui.separator();
                    
                    egui::Frame::new()
                        .fill(Color32::from_rgb(35, 28, 18))
                        .stroke(Stroke::new(2.0, Color32::from_rgb(100, 80, 50)))
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("⎘ Item Details:")
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
                            });
                        });
                }
            }
        });

        ctx.request_repaint();
    }
}

fn format_gp(value: f64) -> String {
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
