// We will add functions to:
// Verify Merkle proofs
// Check historical root consistency
// Validate the audit trail

use iced::{
    widget::{Column, Container, Text, Scrollable, container, Button, Row, TextInput},
    Element, Length, Application, Settings, Color, Alignment,
    theme::{self, Theme},
    Command,
    Border,
    Shadow,
    window,
    Size,
    keyboard::self,
    Subscription,
};
use iced::widget::svg::Svg;
use iced::advanced::svg;
use rocksdb::{DB, Options};
use chrono::{DateTime, Utc, TimeZone};
use std::error::Error;
use std::collections::HashMap;

use config::{Config, File as ConfigFile};

use graymamba::audit_adapters::merkle_tree::MerkleNode;
use graymamba::audit_adapters::irrefutable_audit::AuditEvent;

#[derive(Debug)]
struct ProofData {
    proof_status: Option<bool>,
    consistency_status: Option<bool>,
    audit_trail_status: Option<bool>,
}

struct AuditViewer {
    current_events: String,
    historical_roots: String,
    error_message: Option<String>,
    current_theme: Theme,
    selected_window: Option<i64>,
    window_events: Option<String>,
    verification_status: ProofData,
    selected_event: Option<String>,
    font_size: f32,
    verified_events: HashMap<String, bool>,
    db_path_input: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Message {
    Refresh,
    SelectWindow(i64),
    VerifyProof(String),
    VerifyConsistency,
    VerifyAuditTrail(String),
    SelectEvent(String),
    FontSizeChanged(f32),
    EventVerified(String, bool),
    SetDBPath(String),
    OpenDB,
}

impl Application for AuditViewer {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        let mut viewer = AuditViewer {
            current_events: String::new(),
            historical_roots: String::new(),
            error_message: None,
            current_theme: Theme::Dark,
            selected_window: None,
            window_events: None,
            verification_status: ProofData {
                proof_status: None,
                consistency_status: None,
                audit_trail_status: None,
            },
            selected_event: None,
            font_size: 12.0,
            verified_events: HashMap::new(),
            db_path_input: String::from("../RocksDBs/audit_merkle_db"),
        };
        
        if let Err(e) = viewer.load_audit_data() {
            viewer.error_message = Some(e.to_string());
        }
        
        (viewer, Command::none())
    }

    fn title(&self) -> String {
        String::from("Audit Log Explorer")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::Refresh => {
                self.current_events.clear();
                self.historical_roots.clear();
                self.error_message = None;
                if let Err(e) = self.load_audit_data() {
                    self.error_message = Some(e.to_string());
                }
            }
            Message::SelectWindow(timestamp) => {
                if let Err(e) = self.load_window_events(timestamp) {
                    self.error_message = Some(e.to_string());
                } else {
                    self.selected_window = Some(timestamp);
                }
            }
            
            // New verification message handlers
            Message::VerifyProof(event_key) => {
                match self.verify_merkle_proof(&event_key) {
                    Ok(status) => {
                        self.verification_status.proof_status = Some(status);
                        self.error_message = None;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Proof verification failed: {}", e));
                    }
                }
            }
            Message::VerifyConsistency => {
                match self.verify_historical_consistency() {
                    Ok(status) => {
                        self.verification_status.consistency_status = Some(status);
                        self.error_message = None;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Consistency verification failed: {}", e));
                    }
                }
            }
            Message::VerifyAuditTrail(_event_key) => {
                // Placeholder for future ZK proof implementation
                self.verification_status.audit_trail_status = Some(true);
                self.error_message = None;
            }
            Message::SelectEvent(event_id) => {
                self.selected_event = Some(event_id.clone());
                let result = self.verify_merkle_proof(&event_id);
                self.verified_events.insert(event_id.clone(), result.is_ok());
                //Command::none()
            }
            Message::EventVerified(event_id, result) => {
                self.verified_events.insert(event_id, result);
                //Command::none()
            }
            Message::FontSizeChanged(delta) => {
                self.font_size = (self.font_size + delta).max(8.0);
            }
            Message::SetDBPath(path) => {
                self.db_path_input = path;
            }
            Message::OpenDB => {
                if let Err(e) = self.load_audit_data() {
                    self.error_message = Some(e.to_string());
                }
            }
        }
        Command::none()
    }

    fn theme(&self) -> Theme {
        self.current_theme.clone()
    }

    fn view(&self) -> Element<Message> {
        // Create the side panel first
        let side_panel = Container::new(
            Column::new()
                .width(Length::Fixed(72.0))
                .height(Length::Fill)
                .push(
                    Container::new(
                        Svg::new(svg::Handle::from_path("src/bin/audit_reader/trie.svg"))
                            .width(Length::Fixed(60.0))  // Adjust size as needed
                            .height(Length::Fixed(60.0))
                    )
                    .padding(6)
                    .center_x()
                )
        )
        .style(theme::Container::Custom(Box::new(CustomContainer(Self::hex_to_color("#404040")))))
        .width(Length::Fixed(72.0))
        .height(Length::Fill);

        let _theme_text = match self.current_theme {
            Theme::Light => "Dark",
            Theme::Dark => "Light",
            _ => "Light",
        };
        
        let header = Row::new()
            .width(Length::Fill)
            .align_items(Alignment::Center)
            .push(Text::new("Audit Log Explorer").size(self.font_size * 2.0))
            .push(
                TextInput::new(
                    "Enter DB path...",
                    &self.db_path_input,
                )
                .size(self.font_size)
                .padding(8)
                .on_input(Message::SetDBPath),
            )
            .push(
                Button::new(Text::new("Refresh").size(self.font_size))
                    .padding([4, 8])
                    .on_press(Message::OpenDB)
                    .style(theme::Button::Secondary),
            );

        // Current Events Panel
        let events_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Current Events").size(self.font_size))
                .push(
                    Container::new(
                        Scrollable::new(
                            if self.current_events.is_empty() {
                                Column::new().push(Text::new("No current events available").size(self.font_size))
                            } else {
                                self.current_events
                                    .lines()
                                    .filter(|line| !line.is_empty())
                                    .fold(Column::new().spacing(5), |column, line| {
                                        let parts: Vec<&str> = line.split_whitespace().collect();
                                        if parts.len() >= 4 {
                                            let event_id = parts[0].to_string();
                                            let is_selected = Some(&event_id) == self.selected_event.as_ref();
                                            let verification_status = self.verified_events.get(&event_id);
                                            
                                            column.push(
                                                Container::new(
                                                    Button::new(
                                                        Row::new()
                                                            .spacing(10)
                                                            .push(
                                                                Text::new(
                                                                    if let Some(verified) = verification_status {
                                                                        if *verified {
                                                                            "✓"  // Green tick
                                                                        } else {
                                                                            "✗"  // Red cross
                                                                        }
                                                                    } else {
                                                                        " "  // Not verified yet
                                                                    }
                                                                )
                                                                .size(self.font_size)
                                                                .style(
                                                                    if let Some(verified) = verification_status {
                                                                        if *verified {
                                                                            Color::from_rgb(0.0, 0.8, 0.0)  // Green
                                                                        } else {
                                                                            Color::from_rgb(0.8, 0.0, 0.0)  // Red
                                                                        }
                                                                    } else {
                                                                        Color::from_rgb(0.5, 0.5, 0.5)  // Gray
                                                                    }
                                                                )
                                                            )
                                                            .push(
                                                                Text::new(line)
                                                                    .font(iced::Font::MONOSPACE)
                                                                    .size(self.font_size)
                                                            )
                                                    )
                                                    .style(theme::Button::Text)
                                                    .on_press(Message::SelectEvent(event_id))
                                                )
                                                .width(Length::Fill)
                                                .style(theme::Container::Custom(Box::new(CustomContainer(
                                                    if is_selected {
                                                        Self::hex_to_color("#303030")
                                                    } else {
                                                        Self::hex_to_color("#010101")
                                                    }
                                                ))))
                                                .padding(5)
                                            )
                                        } else {
                                            column
                                        }
                                    })
                            }
                        )
                    )
                    .height(Length::Fill)
                    .padding(10)
                    .style(theme::Container::Custom(Box::new(BorderedContainer)))
                )
        )
        .height(Length::FillPortion(1));

        // Historical Roots Panel
        let roots_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Historical Audits").size(self.font_size))
                .push(
                    Container::new(
                        Scrollable::new(
                            if self.historical_roots.is_empty() {
                                Column::new().push(Text::new("No historical audits available").size(self.font_size))
                            } else {
                                self.historical_roots
                                    .lines()
                                    .filter(|line| !line.is_empty())
                                    .fold(Column::new().spacing(5), |column, line| {
                                        if let Some((ts_str, content)) = line.split_once('|') {
                                            if let Ok(ts) = ts_str.parse::<i64>() {
                                                let is_selected = Some(ts) == self.selected_window;
                                                column.push(
                                                    Container::new(
                                                        Button::new(
                                                            Text::new(content)
                                                                .font(iced::Font::MONOSPACE)
                                                                .size(self.font_size)
                                                        )
                                                        .style(theme::Button::Text)
                                                        .on_press(Message::SelectWindow(ts))
                                                    )
                                                    .width(Length::Fill)
                                                    .style(theme::Container::Custom(Box::new(CustomContainer(
                                                        if is_selected {
                                                            Self::hex_to_color("#303030")
                                                        } else {
                                                            Self::hex_to_color("#010101")
                                                        }
                                                    ))))
                                                    .padding(5)
                                                )
                                            } else {
                                                column
                                            }
                                        } else {
                                            column
                                        }
                                    })
                            }
                        )
                    )
                    .height(Length::Fill)
                    .padding(10)
                    .style(theme::Container::Custom(Box::new(BorderedContainer)))
                )
        )
        .height(Length::FillPortion(1));

        // Historical Details Panel
        let historical_details = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Details of selected Historical Audit").size(self.font_size))
                .push(
                    Container::new(
                        Scrollable::new(
                            if let Some(content) = &self.window_events {
                                Text::new(content)
                                    .font(iced::Font::MONOSPACE)
                                    .size(self.font_size)
                            } else {
                                Text::new("Select a historical audit to view details")
                                    .font(iced::Font::MONOSPACE)
                                    .size(self.font_size)
                            }
                        )
                    )
                    .height(Length::Fill)
                    .padding(10)
                    .style(theme::Container::Custom(Box::new(BorderedContainer)))
                )
        )
        .height(Length::FillPortion(1))
        .width(Length::FillPortion(1));

        // Verification Panel
        let verification_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Verification Controls").size(self.font_size))
                .push(
                    Container::new(
                        Column::new()
                            .spacing(20)
                            .push(
                                Button::new(Text::new("Verify Historical Consistency").size(self.font_size))
                                    .on_press(Message::VerifyConsistency)
                                    .style(theme::Button::Secondary)
                            )
                            .push(
                                Text::new(format!(
                                    "Consistency Status: {}",
                                    self.verification_status.consistency_status
                                        .map(|s| if s { "Consistent" } else { "Inconsistent" })
                                        .unwrap_or("Unknown")
                                )).size(self.font_size)
                            )
                    )
                    .height(Length::Fill)
                    .padding(10)
                    .style(theme::Container::Custom(Box::new(BorderedContainer)))
                )
        )
        .height(Length::FillPortion(1));

        // Create a row for historical audit, verification panel, and details
        let historical_row = Row::new()
            .spacing(20)
            .push(roots_panel.width(Length::FillPortion(2)))          // Historical Audits (left)
            .push(historical_details.width(Length::FillPortion(2)))   // Historical Details (middle)
            .push(verification_panel.width(Length::FillPortion(1)));  // Verification Controls (right)

        let main_content = Column::new()
            .spacing(20)
            .padding(20)
            .max_width(2500)
            .height(Length::Fill)
            .push(header)
            .push(events_panel)
            .push(historical_row);

        // Wrap everything in a row with the side panel
        let content = Row::new()
            .push(side_panel)
            .push(main_content);

        Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::on_key_press(Self::handle_key_press)
    }
}

impl AuditViewer {
    fn handle_key_press(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Message> {
        if modifiers.command() {
            match key {
                keyboard::Key::Character(c) if c == "+" || c == "=" => {
                    Some(Message::FontSizeChanged(2.0))
                }
                keyboard::Key::Character(c) if c == "-" => {
                    Some(Message::FontSizeChanged(-2.0))
                }
                _ => None,
            }
        } else {
            None
        }
    }
    
    fn load_audit_data(&mut self) -> Result<(), Box<dyn Error>> {
        // Load settings but skip logging config since we've already set it up
        let mut settings = Config::default();
        settings
            .merge(ConfigFile::with_name("config/settings.toml"))
            .expect("Failed to load configuration");

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        let cfs = vec!["current_tree", "historical_roots", "event_data", "time_indices"];
        let db = DB::open_cf_for_read_only(
            &opts,
            //settings.get_str("storage.auditdb_path").expect("Failed to get auditdb_path from settings").as_str() ,
            &self.db_path_input,
            &cfs,
            false)?;

        // Load current events into a vector first for sorting
        let mut current_events = Vec::new();
        let cf_current = db.cf_handle("current_tree")
            .ok_or("Failed to get current_tree column family")?;
        
        let iter = db.iterator_cf(cf_current, rocksdb::IteratorMode::End);
        for item in iter {
            let (_, value) = item?;
            let node: MerkleNode = bincode::deserialize(&value)?;
            
            if let Some(event_data) = node.event_data {
                let event: AuditEvent = bincode::deserialize(&event_data)?;
                let timestamp = DateTime::<Utc>::from_timestamp_micros(node.timestamp)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S.%3f UTC")
                    .to_string();
                
                let hash_preview = hex::encode(&node.hash[..4]);
                
                current_events.push((
                    node.timestamp,
                    (
                        hex::encode(&node.hash),
                        format!(
                            "{:<12} {:<24} {:<12} {:<40}\n",
                            format!("{}...", hash_preview),
                            timestamp,
                            event.event_type.to_uppercase(),
                            event.file_path
                        )
                    )
                ));
            }
        }

        // Sort current events by timestamp in reverse order
        current_events.sort_by(|a, b| b.0.cmp(&a.0));
        // Join the sorted events into the display string
        self.current_events = current_events.into_iter()
            .map(|(_, (full_hash, event_str))| format!("{}|{}", full_hash, event_str))
            .collect();

        // Load historical roots into a vector for sorting
        let mut historical_roots = Vec::new();
        let cf_historical = db.cf_handle("historical_roots")
            .ok_or("Failed to get historical_roots column family")?;
        
        let hist_iter = db.iterator_cf(cf_historical, rocksdb::IteratorMode::End);
        for item in hist_iter {
            let (key, value) = item?;
            let window_key = String::from_utf8(key.to_vec())?;
            let root: MerkleNode = bincode::deserialize(&value)?;
            
            let timestamp = window_key.strip_prefix("window:")
                .and_then(|ts| ts.parse::<i64>().ok())
                .and_then(|ts| Utc.timestamp_opt(ts, 0).single())
                .unwrap_or_else(|| Utc::now());
            
            let hash_preview = hex::encode(&root.hash)[..8].to_string();
            
            historical_roots.push((
                timestamp.timestamp(),
                format!(
                    "Window: {}, Root Hash: {}...",
                    timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                    hash_preview
                )
            ));
        }

        // Sort historical roots by timestamp in reverse order
        historical_roots.sort_by(|a, b| b.0.cmp(&a.0));
        
        // Store timestamp and formatted string
        self.historical_roots = historical_roots.into_iter()
            .map(|(ts, root_str)| format!("{}|{}\n", ts, root_str))
            .collect();

        Ok(())
    }

    fn load_window_events(&mut self, timestamp: i64) -> Result<(), Box<dyn Error>> {
        // Load settings but skip logging config since we've already set it up
        let mut settings = Config::default();
        settings
            .merge(ConfigFile::with_name("config/settings.toml"))
            .expect("Failed to load configuration");

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        let cfs = vec!["current_tree", "historical_roots", "event_data", "time_indices"];
        let db = DB::open_cf_for_read_only(
            &opts,
            //settings.get_str("storage.auditdb_path").expect("Failed to get auditdb_path from settings").as_str() ,
            &self.db_path_input,
             &cfs, false)?;

        let cf_historical = db.cf_handle("historical_roots")
            .ok_or("Failed to get historical_roots column family")?;
        
        let window_key = format!("window:{}", timestamp);
        if let Some(value) = db.get_cf(cf_historical, window_key)? {
            let root: MerkleNode = bincode::deserialize(&value)?;
            
            // Format events similar to current window display
            let mut events = Vec::new();
            self.collect_events_from_node(&root, &mut events)?;
            
            // Sort events by timestamp
            events.sort_by(|a, b| b.0.cmp(&a.0));
            
            self.window_events = Some(
                events.into_iter()
                    .map(|(_, event_str)| event_str)
                    .collect()
            );
        }

        Ok(())
    }

    fn collect_events_from_node(
        &self,
        node: &MerkleNode,
        events: &mut Vec<(i64, String)>
    ) -> Result<(), Box<dyn Error>> {
        if let Some(event_data) = &node.event_data {
            let event: AuditEvent = bincode::deserialize(event_data)?;
            let timestamp = DateTime::<Utc>::from_timestamp_micros(node.timestamp)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S.%3f UTC")
                .to_string();
            
            let hash_preview = hex::encode(&node.hash[..4]);
            
            events.push((
                node.timestamp,
                format!(
                    "{:<12} {:<24} {:<12} {:<40}\n",
                    format!("{}...", hash_preview),
                    timestamp,
                    event.event_type.to_uppercase(),
                    event.file_path
                )
            ));
        }

        if let Some(left) = &node.left_child {
            self.collect_events_from_node(left, events)?;
        }
        if let Some(right) = &node.right_child {
            self.collect_events_from_node(right, events)?;
        }

        Ok(())
    }

    fn verify_merkle_proof(&self, event_str: &str) -> Result<bool, Box<dyn Error>> {
        let db = self.open_db()?;
        let cf_current = db.cf_handle("current_tree")
            .ok_or("Failed to get current_tree column family")?;
        
        // Extract full hash from the hidden part of the event string
        let full_hash = event_str.split('|').next()
            .ok_or("Invalid event format")?;
        
        // Convert hex string back to bytes
        let hash_bytes = hex::decode(full_hash)?;
        
        // Look up the event using the key format from merkle_tree.rs
        let iter = db.iterator_cf(cf_current, rocksdb::IteratorMode::Start);
        for item in iter {
            let (_, value_vec) = item?;
            let node: MerkleNode = bincode::deserialize(&value_vec)?;
            if node.hash == hash_bytes {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn verify_historical_consistency(&self) -> Result<bool, Box<dyn Error>> {
        let db = self.open_db()?;
        let cf_historical = db.cf_handle("historical_roots")
            .ok_or("Failed to get historical_roots column family")?;
        
        let mut prev_root: Option<Vec<u8>> = None;
        let mut is_consistent = true;
        
        let hist_iter = db.iterator_cf(cf_historical, rocksdb::IteratorMode::End);
        for item in hist_iter {
            let (_, value_vec) = item?;
            let root: MerkleNode = bincode::deserialize(&value_vec)?;
            
            if let Some(prev) = prev_root {
                if prev == root.hash {
                    is_consistent = false;
                    break;
                }
            }
            prev_root = Some(root.hash);
        }
        
        Ok(is_consistent)
    }

    fn open_db(&self) -> Result<DB, Box<dyn Error>> {
        // Load settings but skip logging config since we've already set it up
        let mut settings = Config::default();
        settings
            .merge(ConfigFile::with_name("config/settings.toml"))
            .expect("Failed to load configuration");
        
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        let cfs = vec!["current_tree", "historical_roots", "event_data", "time_indices"];
        Ok(DB::open_cf_for_read_only(&opts,
            &self.db_path_input,
         &cfs, false)?)
    }
    #[allow(dead_code)]
    fn get_selected_event_key(&self) -> Option<String> {
        // TODO: Add event selection functionality
        // For now, return None
        None
    }

    // Helper function to convert hex to RGB Color
    fn hex_to_color(hex: &str) -> Color {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
        Color::from_rgb(r, g, b)
    }
}

// Add this custom container style
struct BorderedContainer;

impl container::StyleSheet for BorderedContainer {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            text_color: None,
            background: Some(Color::from_rgb(
                0x01 as f32 / 255.0,
                0x01 as f32 / 255.0,
                0x01 as f32 / 255.0,
            ).into()),
            border: Border {
                radius: 5.0.into(),
                width: 1.0,
                color: Color::from_rgb(
                    0x30 as f32 / 255.0,
                    0x30 as f32 / 255.0,
                    0x30 as f32 / 255.0,
                ),
            },
            shadow: Shadow::default(),
        }
    }
}

// Add the CustomContainer struct if not already present:
struct CustomContainer(Color);

impl container::StyleSheet for CustomContainer {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            background: Some(self.0.into()),
            text_color: None,
            border: Border::default(),
            shadow: Shadow::default(),
        }
    }
}

fn main() -> iced::Result {
    AuditViewer::run(Settings {
        window: window::Settings {
            size: Size {width: 1400.0, height: 700.0},  // Width: 1600px, Height: 900px
            position: window::Position::Centered,
            ..window::Settings::default()
        },
        ..Settings::default()
    })
}