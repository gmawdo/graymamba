// We will add functions to:
// Verify Merkle proofs
// Check historical root consistency
// Validate the audit trail

use iced::{
    widget::{Column, Container, Text, Scrollable, container, Button, Row},
    Element, Length, Application, Settings, Color, Alignment,
    alignment,
    theme::{self, Theme},
    Command,
    Border,
    Shadow,
    window,
    Size,
};
use rocksdb::{DB, Options};
use chrono::{DateTime, Utc, TimeZone};
use std::error::Error;

#[cfg(feature = "irrefutable_audit")]
use graymamba::audit_adapters::merkle_tree::MerkleNode;
use graymamba::irrefutable_audit::AuditEvent;

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
}

#[derive(Debug, Clone)]
enum Message {
    ToggleTheme,
    Refresh,
    SelectWindow(i64),
    VerifyProof(String),
    VerifyConsistency,
    #[allow(dead_code)]
    VerifyAuditTrail(String),
    SelectEvent(String),
}

// Add this custom container style
struct ModalContainer;

impl container::StyleSheet for ModalContainer {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            text_color: None,
            background: Some(Color::from_rgb(0.95, 0.95, 0.95).into()),
            border: Border {
                radius: 10.0.into(),
                width: 2.0,
                color: Color::BLACK,
            },
            shadow: Shadow::default(),
        }
    }
}

// Add this overlay style for the background
struct Overlay;

impl container::StyleSheet for Overlay {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            text_color: None,
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.5).into()),
            border: Border::default(),
            shadow: Shadow::default(),
        }
    }
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
            current_theme: Theme::Light,
            selected_window: None,
            window_events: None,
            verification_status: ProofData {
                proof_status: None,
                consistency_status: None,
                audit_trail_status: None,
            },
            selected_event: None,
        };
        
        if let Err(e) = viewer.load_audit_data() {
            viewer.error_message = Some(e.to_string());
        }
        
        (viewer, Command::none())
    }

    fn title(&self) -> String {
        String::from("Audit Log Viewer")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ToggleTheme => {
                self.current_theme = match self.current_theme {
                    Theme::Light => Theme::Dark,
                    Theme::Dark => Theme::Light,
                    _ => Theme::Light,
                };
            }
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
            Message::SelectEvent(event_key) => {
                self.selected_event = Some(event_key);
            }
        }
        Command::none()
    }

    fn theme(&self) -> Theme {
        self.current_theme.clone()
    }

    fn view(&self) -> Element<Message> {
        let theme_text = match self.current_theme {
            Theme::Light => "Dark",
            Theme::Dark => "Light",
            _ => "Light",
        };
        
        let header = Row::new()
            .width(Length::Fill)
            .align_items(Alignment::Center)
            .push(Text::new("Audit Log Viewer").size(24))
            .push(
                Container::new(
                    Row::new()
                        .spacing(8)
                        .push(
                            Button::new(Text::new("Refresh").size(13))
                                .padding([4, 8])
                                .on_press(Message::Refresh)
                                .style(theme::Button::Secondary)
                        )
                        .push(
                            Button::new(Text::new(theme_text).size(13))
                                .padding([4, 8])
                                .on_press(Message::ToggleTheme)
                                .style(theme::Button::Secondary)
                        )
                )
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Right)
            );

        // Current Events Panel
        let events_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Current Events").size(20))
                .push(
                    Container::new(
                        Scrollable::new(
                            if self.current_events.is_empty() {
                                Column::new().push(Text::new("No current events available"))
                            } else {
                                self.current_events
                                    .lines()
                                    .filter(|line| !line.is_empty())
                                    .fold(Column::new().spacing(5), |column, line| {
                                        let parts: Vec<&str> = line.split_whitespace().collect();
                                        if parts.len() >= 4 {
                                            column.push(
                                                Button::new(
                                                    Text::new(line)
                                                        .font(iced::Font::MONOSPACE)
                                                        .size(10)
                                                )
                                                .style(if Some(parts[0].to_string()) == self.selected_event {
                                                    theme::Button::Primary
                                                } else {
                                                    theme::Button::Text
                                                })
                                                .on_press(Message::SelectEvent(parts[0].to_string()))
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
                .push(Text::new("Historical Audits").size(20))
                .push(
                    Container::new(
                        Scrollable::new(
                            if self.historical_roots.is_empty() {
                                Column::new().push(Text::new("No historical audits available"))
                            } else {
                                self.historical_roots
                                    .lines()
                                    .filter(|line| !line.is_empty())
                                    .fold(Column::new().spacing(5), |column, line| {
                                        if let Some((ts_str, content)) = line.split_once('|') {
                                            if let Ok(ts) = ts_str.parse::<i64>() {
                                                return column.push(
                                                    Row::new()
                                                        .spacing(10)
                                                        .push(
                                                            Text::new(content)
                                                                .font(iced::Font::MONOSPACE)
                                                                .size(10)
                                                        )
                                                        .push(
                                                            Button::new(Text::new("View").size(13))
                                                                .padding([4, 8])
                                                                .on_press(Message::SelectWindow(ts))
                                                                .style(theme::Button::Secondary)
                                                        )
                                                );
                                            }
                                        }
                                        column.push(
                                            Text::new(line)
                                                .font(iced::Font::MONOSPACE)
                                                .size(10)
                                        )
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
                .push(Text::new("Historical Details").size(20))
                .push(
                    Container::new(
                        Scrollable::new(
                            if let Some(content) = &self.window_events {
                                Text::new(content)
                                    .font(iced::Font::MONOSPACE)
                                    .size(10)  // Smaller font size
                            } else {
                                Text::new("Select a historical audit to view details")
                                    .font(iced::Font::MONOSPACE)
                                    .size(10)
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

        // Create a row for historical audit and details
        let historical_row = Row::new()
            .spacing(20)
            .push(roots_panel.width(Length::FillPortion(1)))
            .push(historical_details);

        // Define verification controls
        let verification_controls = Column::new()
            .spacing(10)
            .push(
                Button::new(Text::new("Verify Selected Event"))
                    .on_press(Message::VerifyProof(
                        self.selected_event.clone().unwrap_or_default()
                    ))
                    .style(theme::Button::Secondary)
            )
            .push(
                Button::new(Text::new("Verify Historical Consistency"))
                    .on_press(Message::VerifyConsistency)
                    .style(theme::Button::Secondary)
            );

        // Define status display
        let status_display = Column::new()
            .spacing(10)
            .push(
                Text::new(format!(
                    "Proof Status: {}",
                    self.verification_status.proof_status
                        .map(|s| if s { "Valid" } else { "Invalid" })
                        .unwrap_or("Unknown")
                ))
            )
            .push(
                Text::new(format!(
                    "Consistency Status: {}",
                    self.verification_status.consistency_status
                        .map(|s| if s { "Consistent" } else { "Inconsistent" })
                        .unwrap_or("Unknown")
                ))
            );

        // Verification Panel
        let verification_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Verification Controls").size(20))
                .push(
                    Container::new(
                        Column::new()
                            .spacing(20)
                            .push(verification_controls)
                            .push(status_display)
                    )
                    .height(Length::Fill)
                    .padding(10)
                    .style(theme::Container::Custom(Box::new(BorderedContainer)))
                )
        )
        .height(Length::FillPortion(1));

        let mut content = Column::new()
            .spacing(20)
            .padding(20)
            .max_width(2500)  // Increased max width to accommodate the extra panel
            .height(Length::Fill)
            .push(header)
            .push(events_panel)
            .push(historical_row)  // Use the row instead of just roots_panel
            .push(verification_panel);

        // Add error message if present
        if let Some(error) = &self.error_message {
            content = content.push(
                Text::new(error)
                    .style(Color::from_rgb(0.8, 0.0, 0.0))
            );
        }

        Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
    }
}

impl AuditViewer {
    fn load_audit_data(&mut self) -> Result<(), Box<dyn Error>> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        let cfs = vec!["current_tree", "historical_roots", "event_data", "time_indices"];
        let db = DB::open_cf_for_read_only(&opts, "../RocksDBs/audit_merkle_db", &cfs, false)?;

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
                    .format("%Y-%m-%d %H:%M:%S UTC")
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
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        let cfs = vec!["current_tree", "historical_roots", "event_data", "time_indices"];
        let db = DB::open_cf_for_read_only(&opts, "../RocksDBs/audit_merkle_db", &cfs, false)?;

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
                .format("%Y-%m-%d %H:%M:%S UTC")
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
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        let cfs = vec!["current_tree", "historical_roots", "event_data", "time_indices"];
        Ok(DB::open_cf_for_read_only(&opts, "../RocksDBs/audit_merkle_db", &cfs, false)?)
    }
    #[allow(dead_code)]
    fn get_selected_event_key(&self) -> Option<String> {
        // TODO: Add event selection functionality
        // For now, return None
        None
    }
}

// Add this custom container style
struct BorderedContainer;

impl container::StyleSheet for BorderedContainer {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            text_color: None,
            background: Some(Color::from_rgb(0.95, 0.95, 0.95).into()),
            border: Border {
                radius: 5.0.into(),
                width: 1.0,
                color: Color::BLACK,
            },
            shadow: Shadow::default(),
        }
    }
}

fn main() -> iced::Result {
    AuditViewer::run(Settings {
        window: window::Settings {
            size: Size {width: 1400.0, height: 900.0},  // Width: 1600px, Height: 900px
            position: window::Position::Centered,
            ..window::Settings::default()
        },
        ..Settings::default()
    })
}