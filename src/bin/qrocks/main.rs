use iced::{
    widget::{Column, Container, Text, Scrollable, Button, Row, TextInput},
    Element, Length, Application, Settings, Color, Alignment,
    theme::{self, Theme, Palette},
    Command,
    window,
    Size,
    Subscription,
    keyboard,
    widget::container,
};
use iced::widget::svg::Svg;
use iced::advanced::svg;
use rocksdb::{DB, Options};
use std::error::Error;
use config::{Config, File as ConfigFile};

struct CustomContainer(Color);

impl container::StyleSheet for CustomContainer {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            background: Some(self.0.into()),
            text_color: Some(Color::WHITE),
            ..Default::default()
        }
    }
}

#[derive(Debug)]
struct DBExplorer {
    db_path: String,
    db_path_input: String,
    current_db: Option<DB>,
    keys: Vec<String>,
    values: Vec<String>,
    error_message: Option<String>,
    selected_key: Option<String>,
    filter_text: String,
    font_size: f32,
}

#[derive(Debug, Clone)]
enum Message {
    OpenDB,
    SetDBPath(String),
    SelectKey(String),
    FilterChanged(String),
    Refresh,
    FontSizeChanged(f32),
}

impl Application for DBExplorer {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        // Load settings but skip logging config since we've already set it up
        let mut settings = Config::default();
        settings
            .merge(ConfigFile::with_name("config/settings.toml"))
            .expect("Failed to load configuration");

        let default_db_path = settings.get_str("storage.rocksdb_path")
            .expect("Failed to get rocksdb_path from settings");

        (
            DBExplorer {
                db_path: String::new(),
                db_path_input: default_db_path,
                current_db: None,
                keys: Vec::new(),
                values: Vec::new(),
                error_message: None,
                selected_key: None,
                filter_text: String::new(),
                font_size: 12.0,
            },
            Command::perform(async {}, |_| Message::OpenDB)
        )
    }

    fn title(&self) -> String {
        String::from("RocksDB Explorer")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
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
                self.values.clear();
                self.selected_key = None;
                if let Err(e) = self.load_db_data() {
                    self.error_message = Some(e.to_string());
                }
            }
            Message::FontSizeChanged(delta) => {
                self.font_size = (self.font_size + delta).max(8.0);
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        let header = Row::new()
            .spacing(10)
            .align_items(Alignment::Center)
            .push(Text::new("RocksDB Explorer").size(self.font_size * 2.0))
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
                Button::new(Text::new("Open DB").size(self.font_size))
                    .padding([4, 8])
                    .on_press(Message::OpenDB)
                    .style(theme::Button::Primary),
            )
            .push(
                Button::new(Text::new("Refresh").size(self.font_size))
                    .padding([4, 8])
                    .on_press(Message::Refresh)
                    .style(theme::Button::Secondary),
            );

        let filter = TextInput::new(
            "Filter keys...",
            &self.filter_text
        )
        .size(self.font_size)
        .padding(8)
        .on_input(Message::FilterChanged);

        let keys_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Keys").size(self.font_size))
                .push(filter)
                .push(
                    Container::new(
                        Scrollable::new(
                            self.keys
                                .iter()
                                .fold(Column::new().spacing(5), |column, key| {
                                    let is_selected = Some(key.to_string()) == self.selected_key;
                                    column.push(
                                        Container::new(
                                            Button::new(
                                                Text::new(key)
                                                    .size(self.font_size)
                                                    .font(iced::Font::MONOSPACE),
                                            )
                                            .style(theme::Button::Text)
                                            .on_press(Message::SelectKey(key.to_string()))
                                        )
                                        .width(Length::Fill)
                                        .style(theme::Container::Custom(Box::new(CustomContainer(
                                            if is_selected {
                                                Self::hex_to_color("#303030")  // Soft gray for selected row
                                            } else {
                                                Self::hex_to_color("#202020")  // Default background
                                            }
                                        ))))
                                        .padding(5)
                                    )
                                }),
                        ),
                    )
                    .height(Length::Fill)
                    .style(theme::Container::Custom(Box::new(CustomContainer(Self::hex_to_color("#202020")))))
                ),
        )
        .width(Length::FillPortion(1));

        let values_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Values").size(self.font_size))
                .push(
                    Container::new(
                        Scrollable::new(
                            self.values
                                .iter()
                                .fold(Column::new().spacing(5), |column, value| {
                                    column.push(
                                        Container::new(
                                            Text::new(Self::format_value_with_wrapping(value, 70))
                                                .size(self.font_size)
                                                .font(iced::Font::MONOSPACE),
                                        )
                                        .width(Length::Fill)
                                        .style(theme::Container::Custom(Box::new(CustomContainer(Self::hex_to_color("#202020")))))
                                        .padding(10)
                                    )
                                }),
                        )
                        .width(Length::Fill),
                    )
                    .height(Length::Fill)
                    .style(theme::Container::Custom(Box::new(CustomContainer(Self::hex_to_color("#202020")))))
                ),
        )
        .width(Length::FillPortion(1));

        let side_panel = Container::new(
            Column::new()
                .width(Length::Fixed(72.0))
                .height(Length::Fill)
                .push(
                    Container::new(
                        Svg::new(svg::Handle::from_path("src/bin/qrocks/RocksDB.svg"))
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

        let main_content = Column::new()
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

        let content = Row::new()
            .push(side_panel)
            .push(main_content);

        if let Some(error) = &self.error_message {
            Container::new(
                Column::new()
                    .push(content)
                    .push(
                        Text::new(error)
                            .style(Color::from_rgb(0.8, 0.0, 0.0)),
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
        Theme::custom(String::from ("graymamba"), Palette {
            background: Self::hex_to_color("#010101"),  // Dark gray background
            text: Color::WHITE,
            primary: Self::hex_to_color("#202020"),     // Lighter gray for primary elements
            success: Self::hex_to_color("#00FF00"),
            danger: Self::hex_to_color("#FF0000"),
        })
    }
 
    fn subscription(&self) -> Subscription<Message> {
        keyboard::on_key_press(Self::handle_key_press)
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

    fn format_value_with_wrapping(value: &str, chars_per_line: usize) -> String {
        value.chars()
            .collect::<Vec<char>>()
            .chunks(chars_per_line)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect::<Vec<String>>()
            .join("\n")
    }
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

    // Helper function to convert hex to RGB Color
    fn hex_to_color(hex: &str) -> Color {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
        Color::from_rgb(r, g, b)
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
        default_font: iced::Font::DEFAULT,
        default_text_size: iced::Pixels(16.0),
        ..Settings::default()
    })
}