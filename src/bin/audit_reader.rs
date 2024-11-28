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
};
use rocksdb::{DB, Options};
use chrono::{DateTime, Utc, TimeZone};
use std::error::Error;

#[cfg(feature = "irrefutable_audit")]
use graymamba::audit_adapters::merkle_tree::MerkleNode;
use graymamba::irrefutable_audit::AuditEvent;

struct AuditViewer {
    current_events: String,
    historical_roots: String,
    error_message: Option<String>,
    current_theme: Theme,
}

#[derive(Debug, Clone)]
enum Message {
    ToggleTheme,
    Refresh,
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
                // Clear existing data
                self.current_events.clear();
                self.historical_roots.clear();
                self.error_message = None;

                // Reload data
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

        let events_area = Container::new(
            Scrollable::new(
                Text::new(&self.current_events)
                    .font(iced::Font::MONOSPACE)
                    .size(12)
            )
            .width(Length::Fill)
        )
        .width(Length::Fill)
        .height(Length::FillPortion(2))
        .padding(10)
        .style(theme::Container::Custom(Box::new(BorderedContainer)));

        let roots_area = Container::new(
            Scrollable::new(
                Text::new(&self.historical_roots)
                    .font(iced::Font::MONOSPACE)
                    .size(12)
            )
            .width(Length::Fill)
        )
        .width(Length::Fill)
        .height(Length::FillPortion(1))
        .padding(10)
        .style(theme::Container::Custom(Box::new(BorderedContainer)));

        let content = Column::new()
            .spacing(20)
            .padding(20)
            .max_width(2000)
            .height(Length::Fill)
            .push(header)
            .push(events_area)
            .push(Text::new("Historical Audits").size(24))
            .push(roots_area);

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
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
                
                let hash_preview = hex::encode(&node.hash[..4]);
                
                current_events.push((
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
        }

        // Sort current events by timestamp in reverse order
        current_events.sort_by(|a, b| b.0.cmp(&a.0));
        // Join the sorted events into the display string
        self.current_events = current_events.into_iter()
            .map(|(_, event_str)| event_str)
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
            
            historical_roots.push((
                timestamp.timestamp(),
                format!(
                    "Window: {}, Root Hash: {}\n",
                    timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                    hex::encode(&root.hash)
                )
            ));
        }

        // Sort historical roots by timestamp in reverse order
        historical_roots.sort_by(|a, b| b.0.cmp(&a.0));
        // Join the sorted roots into the display string
        self.historical_roots = historical_roots.into_iter()
            .map(|(_, root_str)| root_str)
            .collect();

        Ok(())
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