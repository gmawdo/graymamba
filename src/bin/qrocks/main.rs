use iced::{
    widget::{Column, Container, Text, Scrollable, Button, Row, TextInput},
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
use std::error::Error;

#[derive(Debug)]
struct DBExplorer {
    db_path: String,
    db_path_input: String,
    current_db: Option<DB>,
    keys: Vec<String>,
    values: Vec<String>,
    error_message: Option<String>,
    current_theme: Theme,
    selected_key: Option<String>,
    filter_text: String,
}

#[derive(Debug, Clone)]
enum Message {
    ToggleTheme,
    OpenDB,
    SetDBPath(String),
    SelectKey(String),
    FilterChanged(String),
    Refresh,
}

impl Application for DBExplorer {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (
            DBExplorer {
                db_path: String::new(),
                db_path_input: String::from("../RocksDBs/yellowduck"),
                current_db: None,
                keys: Vec::new(),
                values: Vec::new(),
                error_message: None,
                current_theme: Theme::Light,
                selected_key: None,
                filter_text: String::new(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("RocksDB Explorer")
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
            Message::OpenDB => {
                self.db_path = self.db_path_input.clone();
                if let Err(e) = self.load_db_data() {
                    self.error_message = Some(e.to_string());
                }
            }
            Message::SetDBPath(path) => {
                self.db_path_input = path;
            }
            Message::SelectKey(key) => {
                self.selected_key = Some(key);
                if let Err(e) = self.load_value_for_key() {
                    self.error_message = Some(e.to_string());
                }
            }
            Message::FilterChanged(text) => {
                self.filter_text = text;
                if let Err(e) = self.load_db_data() {
                    self.error_message = Some(e.to_string());
                }
            }
            Message::Refresh => {
                if let Err(e) = self.load_db_data() {
                    self.error_message = Some(e.to_string());
                }
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let header = Row::new()
            .spacing(10)
            .align_items(Alignment::Center)
            .push(Text::new("RocksDB Explorer").size(24))
            .push(
                TextInput::new(
                    "Enter DB path...",
                    &self.db_path_input,
                )
                .padding(8)
                .size(16),
            )
            .push(
                Button::new(Text::new("Open DB"))
                    .padding([4, 8])
                    .on_press(Message::OpenDB)
                    .style(theme::Button::Primary),
            )
            .push(
                Button::new(Text::new("Refresh"))
                    .padding([4, 8])
                    .on_press(Message::Refresh)
                    .style(theme::Button::Secondary),
            )
            .push(
                Button::new(Text::new(if matches!(self.current_theme, Theme::Light) {
                    "Dark"
                } else {
                    "Light"
                }))
                .padding([4, 8])
                .on_press(Message::ToggleTheme)
                .style(theme::Button::Secondary),
            );

        let filter = TextInput::new(
            "Filter keys...",
            &self.filter_text
        )
        .padding(8)
        .size(16);

        let keys_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Keys").size(20))
                .push(filter)
                .push(
                    Container::new(
                        Scrollable::new(
                            self.keys
                                .iter()
                                .fold(Column::new().spacing(5), |column, key| {
                                    column.push(
                                        Button::new(
                                            Text::new(key)
                                                .font(iced::Font::MONOSPACE)
                                                .size(12),
                                        )
                                        .style(if Some(key.to_string())
                                            == self.selected_key
                                        {
                                            theme::Button::Primary
                                        } else {
                                            theme::Button::Text
                                        })
                                        .on_press(Message::SelectKey(key.to_string())),
                                    )
                                }),
                        ),
                    )
                    .height(Length::Fill)
                    .style(theme::Container::Box),
                ),
        )
        .width(Length::FillPortion(1));

        let values_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Values").size(20))
                .push(
                    Container::new(
                        Scrollable::new(
                            self.values
                                .iter()
                                .fold(Column::new().spacing(5), |column, value| {
                                    column.push(
                                        Text::new(value)
                                            .font(iced::Font::MONOSPACE)
                                            .size(12),
                                    )
                                }),
                        ),
                    )
                    .height(Length::Fill)
                    .style(theme::Container::Box),
                ),
        )
        .width(Length::FillPortion(1));

        let content = Column::new()
            .spacing(20)
            .padding(20)
            .max_width(1200)
            .height(Length::Fill)
            .push(header)
            .push(
                Row::new()
                    .spacing(20)
                    .height(Length::Fill)
                    .push(keys_panel)
                    .push(values_panel),
            );

        if let Some(error) = &self.error_message {
            Container::new(
                Column::new()
                    .push(content)
                    .push(
                        Text::new(error)
                            .style(Color::from_rgb(0.8, 0.0, 0.0))
                            .size(16),
                    ),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
        } else {
            Container::new(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x()
                .into()
        }
    }

    fn theme(&self) -> Theme {
        self.current_theme.clone()
    }
}

impl DBExplorer {
    fn load_db_data(&mut self) -> Result<(), Box<dyn Error>> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        
        self.current_db = Some(DB::open_for_read_only(&opts, &self.db_path, false)?);
        
        if let Some(db) = &self.current_db {
            let mut keys = Vec::new();
            let iter = db.iterator(rocksdb::IteratorMode::Start);
            
            for item in iter {
                let (key, _) = item?;
                let key_str = String::from_utf8(key.to_vec())?;
                
                if self.filter_text.is_empty() 
                    || key_str.to_lowercase().contains(&self.filter_text.to_lowercase()) {
                    keys.push(key_str);
                }
            }
            
            self.keys = keys;
            self.error_message = None;
        }
        
        Ok(())
    }

    fn load_value_for_key(&mut self) -> Result<(), Box<dyn Error>> {
        self.values.clear();
        
        if let Some(db) = &self.current_db {
            if let Some(key) = &self.selected_key {
                if let Some(value) = db.get(key.as_bytes())? {
                    let value_str = String::from_utf8(value)?;
                    self.values.push(value_str);
                }
            }
        }
        
        Ok(())
    }
}

fn main() -> iced::Result {
    DBExplorer::run(Settings {
        window: window::Settings {
            size: Size {
                width: 1200.0,
                height: 800.0,
            },
            position: window::Position::Centered,
            ..window::Settings::default()
        },
        ..Settings::default()
    })
}