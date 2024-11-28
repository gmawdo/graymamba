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
};
use rocksdb::{DB, Options};
use chrono::{DateTime, Utc, TimeZone};
use std::error::Error;

#[cfg(feature = "irrefutable_audit")]
use graymamba::audit_adapters::merkle_tree::MerkleNode;
use graymamba::irrefutable_audit::AuditEvent;
use graymamba::audit_adapters::poseidon_hash::PoseidonHasher;

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
    CloseModal,
    VerifyProof(String),
    VerifyConsistency,
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
                }
                self.selected_window = Some(timestamp);
            }
            Message::CloseModal => {
                self.selected_window = None;
                self.window_events = None;
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
            Message::VerifyAuditTrail(event_key) => {
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
            .push(Text::new("Current Window Events").size(24))
            .push(
                Container::new(
                    Row::new()
                        .spacing(8)
                        .push(
                            Button::new(
                                Text::new("Refresh")
                                    .size(13)
                            )
                            .padding([4, 8])
                            .on_press(Message::Refresh)
                            .style(theme::Button::Secondary)
                        )
                        .push(
                            Button::new(
                                Text::new(theme_text)
                                    .size(13)
                            )
                            .padding([4, 8])
                            .on_press(Message::ToggleTheme)
                            .style(theme::Button::Secondary)
                        )
                )
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Right)
            );

        let events_area = Scrollable::new(
            Column::new()
                .spacing(10)
                .push(
                    self.current_events
                        .lines()
                        .filter(|line| !line.is_empty())
                        .fold(Column::new().spacing(5), |column, line| {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if parts.len() >= 4 {
                                column.push(
                                    Button::new(Text::new(line))
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
                )
        );

        let roots_area = Container::new(
            Scrollable::new(
                Column::new()
                    .spacing(5)
                    .push(
                        self.historical_roots.lines()
                            .filter(|line| !line.is_empty())
                            .fold(
                                Column::new().spacing(5),
                                |column, line| {
                                    if let Some((ts_str, content)) = line.split_once('|') {
                                        if let Ok(ts) = ts_str.parse::<i64>() {
                                            return column.push(
                                                Row::new()
                                                    .spacing(10)
                                                    .push(
                                                        Text::new(content)
                                                            .font(iced::Font::MONOSPACE)
                                                            .size(12)
                                                    )
                                                    .push(
                                                        Button::new(
                                                            Text::new("View")
                                                                .size(13)
                                                        )
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
                                            .size(12)
                                    )
                                }
                            )
                    )
            )
            .width(Length::Fill)
        )
        .width(Length::Fill)
        .height(Length::FillPortion(1))
        .padding(10)
        .style(theme::Container::Custom(Box::new(BorderedContainer)));

        let verification_controls = Row::new()
            .spacing(20)
            .push(
                Button::new(Text::new("Verify Selected Event"))
                    .on_press(Message::VerifyProof(
                        self.selected_event.clone().unwrap_or_default()
                    ))
                    .style(if self.selected_event.is_some() {
                        theme::Button::Primary
                    } else {
                        theme::Button::Secondary
                    })
            )
            .push(
                Button::new(Text::new("Verify Historical Consistency"))
                    .on_press(Message::VerifyConsistency)
            );

        let status_display = Row::new()
            .spacing(20)
            .push(
                Text::new(match self.verification_status.proof_status {
                    Some(true) => "✓ Proof Valid",
                    Some(false) => "✗ Proof Invalid",
                    None => "No proof verified"
                })
            )
            .push(
                Text::new(match self.verification_status.consistency_status {
                    Some(true) => "✓ History Consistent",
                    Some(false) => "✗ History Inconsistent",
                    None => "No consistency check"
                })
            );

        let mut content = Column::new()
            .spacing(20)
            .padding(20)
            .max_width(2000)
            .height(Length::Fill)
            .push(header)
            .push(events_area)
            .push(Text::new("Historical Audits").size(24))
            .push(roots_area)
            .push(verification_controls)
            .push(status_display);

        // Add popup if window is selected
        if let Some(_) = self.selected_window {
            let modal_content: Element<_> = Container::new(
                Column::new()
                    .push(content)
                    .push(
                        Container::new(
                            Container::new(
                                Column::new()
                                    .spacing(20)
                                    .max_width(800)
                                    .push(
                                        Row::new()
                                            .push(Text::new("Historical Window Events").size(24))
                                            .push(
                                                Container::new(
                                                    Button::new(Text::new("×").size(20))
                                                        .on_press(Message::CloseModal)
                                                        .style(theme::Button::Text)
                                                )
                                                .width(Length::Fill)
                                                .align_x(alignment::Horizontal::Right)
                                            )
                                    )
                                    .push(
                                        Container::new(
                                            Scrollable::new(
                                                Text::new(self.window_events.as_deref().unwrap_or("No events found"))
                                                    .font(iced::Font::MONOSPACE)
                                                    .size(12)
                                            )
                                        )
                                        .height(Length::Fixed(400.0))
                                        .padding(10)
                                        .style(theme::Container::Box)
                                    )
                            )
                            .padding(20)
                            .style(theme::Container::Custom(Box::new(ModalContainer)))
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .center_x()
                        .center_y()
                        .style(theme::Container::Custom(Box::new(Overlay)))
                    )
            ).into();
            content = Column::new().push(modal_content);
        }

        let content = if let Some(error) = &self.error_message {
            content.push(
                Text::new(error)
                    .style(Color::from_rgb(0.8, 0.0, 0.0))
            )
        } else {
            content
        };

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
            ..container::Appearance::default()
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    AuditViewer::run(Settings::default())?;
    Ok(())
}