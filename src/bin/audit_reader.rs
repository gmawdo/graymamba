// We will add functions to:
// Verify Merkle proofs
// Check historical root consistency
// Validate the audit trail

use iced::{
    widget::{Column, Container, Text, Scrollable, container, Button, Row},
    Element, Length, Sandbox, Settings, Color, Alignment,
    alignment,
    Theme,
    theme::self,
};
use rocksdb::{DB, Options};
use chrono::{DateTime, Utc};
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
}

impl Sandbox for AuditViewer {
    type Message = Message;

    fn new() -> Self {
        let mut viewer = AuditViewer {
            current_events: String::new(),
            historical_roots: String::new(),
            error_message: None,
            current_theme: Theme::Light,
        };
        
        // Load data on startup
        if let Err(e) = viewer.load_audit_data() {
            viewer.error_message = Some(e.to_string());
        }
        
        viewer
    }

    fn title(&self) -> String {
        String::from("Audit Log Viewer")
    }

    fn theme(&self) -> Theme {
        self.current_theme.clone()
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::ToggleTheme => {
                self.current_theme = match self.current_theme {
                    Theme::Light => Theme::Dark,
                    Theme::Dark => Theme::Light,
                    _ => Theme::Dark,
                };
            }
        }
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
                    Button::new(theme_text)
                        .on_press(Message::ToggleTheme)
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
            .push(Text::new("Historical Root Hashes").size(24))
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

        // Load current events
        let cf_current = db.cf_handle("current_tree")
            .ok_or("Failed to get current_tree column family")?;
        
        // Similar to your print_events function, but append to String instead
        let iter = db.iterator_cf(cf_current, rocksdb::IteratorMode::Start);
        for item in iter {
            let (_, value) = item?;
            let node: MerkleNode = bincode::deserialize(&value)?;
            
            if let Some(event_data) = node.event_data {
                let event: AuditEvent = bincode::deserialize(&event_data)?;
                let timestamp = DateTime::<Utc>::from_timestamp_micros(node.timestamp)
                    .unwrap()
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
                
                let hash_preview = hex::encode(&node.hash[..4]);
                
                self.current_events.push_str(&format!(
                    "{:<12} {:<24} {:<12} {:<40}\n",
                    format!("{}...", hash_preview),
                    timestamp,
                    event.event_type.to_uppercase(),
                    event.file_path
                ));
            }
        }

        // Load historical roots
        let cf_historical = db.cf_handle("historical_roots")
            .ok_or("Failed to get historical_roots column family")?;
        
        let hist_iter = db.iterator_cf(cf_historical, rocksdb::IteratorMode::Start);
        for item in hist_iter {
            let (key, value) = item?;
            let window_key = String::from_utf8(key.to_vec())?;
            let root: MerkleNode = bincode::deserialize(&value)?;
            self.historical_roots.push_str(&format!(
                "Window: {}, Root Hash: {}\n",
                window_key,
                hex::encode(&root.hash)
            ));
        }

        Ok(())
    }
}

// Add this custom container style
struct BorderedContainer;

impl container::StyleSheet for BorderedContainer {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            border_width: 1.0,
            border_color: Color::BLACK,
            ..container::Appearance::default()
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    AuditViewer::run(Settings::default())?;
    Ok(())
}