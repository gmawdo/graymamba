use iced::{
    widget::{Column, Container, Text, Scrollable, Button, Row, TextInput},
    Element, Length, Application, Settings, Color, Alignment,
    theme::{self, Theme, Palette},
    Command,
    Border,
    Shadow,
    window,
    Size,
    keyboard::{self, Key, Modifiers},
    Subscription,
    widget::container,
};
use iced::widget::svg::Svg;
use iced::advanced::svg;
use std::error::Error;
use url::Url;
use rocksdb::{DB, Options};
use std::sync::Arc;
use graymamba::sharesfs::SharesFS;
use graymamba::kernel::protocol::tcp::NFSTcpListener;
//use graymamba::kernel::client::NFSClient;

use tracing::debug;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};
use config::{Config, File as ConfigFile};

use tokio::net::TcpStream;
use std::net::SocketAddr;
// State for login modal
#[derive(Debug, Default)]
struct LoginState {
    username: String,
    password: String,
    error: Option<String>,
    is_visible: bool,
}

#[derive(Debug)]
struct DataRoom {
    login_state: LoginState,
    authenticated_user: Option<String>,
    files: Vec<FileEntry>,
    error_message: Option<String>,
    font_size: f32,
    //nfs_client: Option<NFSClient>,
}

#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    size: u64,
    modified: String,
}

#[derive(Debug, Clone)]
enum Message {
    ShowLogin,
    CloseLogin,
    UpdateUsername(String),
    UpdatePassword(String),
    AttemptLogin,
    Logout,
    RefreshFiles,
    FontSizeChanged(f32),
    Login,
}

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

impl Application for DataRoom {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (
            DataRoom {
                login_state: LoginState::default(),
                authenticated_user: None,
                files: Vec::new(),
                error_message: None,
                font_size: 12.0,
                //nfs_client: None,
            },
            Command::none()
        )
    }

    fn title(&self) -> String {
        String::from("Data Room")
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::ShowLogin => {
                self.login_state.is_visible = true;
            }
            Message::CloseLogin => {
                self.login_state.is_visible = false;
                self.login_state.error = None;
            }
            Message::UpdateUsername(username) => {
                self.login_state.username = username;
            }
            Message::UpdatePassword(password) => {
                self.login_state.password = password;
            }
            Message::AttemptLogin => {
                if self.login_state.username.is_empty() || self.login_state.password.is_empty() {
                    self.login_state.error = Some("Username and password are required".to_string());
                    return Command::none();
                }

                // Here we would normally validate credentials against a backend
                // For now, we'll just accept any non-empty credentials
                let username = self.login_state.username.clone();
                self.authenticated_user = Some(username.clone());
                
                // TODO: Connect to NFS
            }
            Message::Logout => {
                self.authenticated_user = None;
                //self.nfs_client = None;
                self.files.clear();
            }
            Message::RefreshFiles => {
                debug!("=== Refreshing files ===");

            }
            Message::FontSizeChanged(delta) => {
                self.font_size = (self.font_size + delta).max(8.0);
            }
            Message::Login => {
                // Here we would normally validate credentials against a backend
                // For now, we'll just accept any non-empty credentials
                let username = self.login_state.username.clone();
                self.authenticated_user = Some(username.clone());
                
                // TODO Connect to NFS
            }
        }
        Command::none()
    }

    fn view(&self) -> Element<Message> {
        if self.login_state.is_visible {
            self.view_login_modal()
        } else {
            let refresh_button = Button::new(
                Text::new("🔄 Refresh")
                    .size(self.font_size)
            )
            .on_press(Message::RefreshFiles)
            .padding(10);

            let header = Row::new()
                .align_items(Alignment::Center)
                .spacing(10)
                .push(refresh_button)
                .push(
                    if self.authenticated_user.is_none() {
                        Button::new(Text::new("Login").size(self.font_size))
                            .padding([4, 8])
                            .on_press(Message::ShowLogin)
                            .style(theme::Button::Primary)
                    } else {
                        Button::new(Text::new("Logout").size(self.font_size))
                            .padding([4, 8])
                            .on_press(Message::Logout)
                            .style(theme::Button::Secondary)
                    }
                );

            let side_panel = Container::new(
                Column::new()
                    .width(Length::Fixed(72.0))
                    .height(Length::Fill)
                    .push(
                        Container::new(
                            Svg::new(svg::Handle::from_path("src/bin/qrocks/RocksDB.svg"))
                                .width(Length::Fixed(60.0))
                                .height(Length::Fixed(60.0))
                        )
                        .padding(6)
                        .center_x()
                    )
            )
            .style(theme::Container::Custom(Box::new(CustomContainer(Self::hex_to_color("#404040")))))
            .width(Length::Fixed(72.0))
            .height(Length::Fill);

            let files_panel = Container::new(
                Column::new()
                    .spacing(10)
                    .push(Text::new("Your Files").size(self.font_size))
                    .push(
                        Container::new(
                            Scrollable::new(
                                Column::new()
                                    .spacing(5)
                                    .push(Text::new(format!("Connected as: {}", 
                                        self.authenticated_user.as_ref().unwrap_or(&"Guest".to_string()))))
                            )
                        )
                        .width(Length::Fixed(700.0))
                        .height(Length::Fixed(700.0))
                        .padding(10)
                        .style(theme::Container::Custom(Box::new(BorderedContainer)))
                    )
            )
            .height(Length::Fill)
            .width(Length::Fill);

            let main_content = Column::new()
                .spacing(20)
                .padding(20)
                .max_width(1200)
                .height(Length::Fill)
                .push(header)
                .push(files_panel);

            let content = Row::new()
                .push(side_panel)
                .push(main_content);

            Container::new(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x()
                .into()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::on_key_press(|key, modifiers| {
            if modifiers.command() {
                match key {
                    Key::Character(c) if c == "+" || c == "=" => {
                        Some(Message::FontSizeChanged(2.0))
                    }
                    Key::Character(c) if c == "-" => {
                        Some(Message::FontSizeChanged(-2.0))
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
    }
}

impl DataRoom {
    fn view_main_content(&self) -> Element<Message> {
        let header = Row::new()
            .spacing(10)
            .align_items(Alignment::Center)
            .push(Text::new("Data Room").size(self.font_size * 2.0))
            .push(
                if self.authenticated_user.is_none() {
                    Button::new(Text::new("Login").size(self.font_size))
                        .padding([4, 8])
                        .on_press(Message::ShowLogin)
                        .style(theme::Button::Primary)
                } else {
                    Button::new(Text::new("Logout").size(self.font_size))
                        .padding([4, 8])
                        .on_press(Message::Logout)
                        .style(theme::Button::Secondary)
                }
            );

        let side_panel = Container::new(
            Column::new()
                .width(Length::Fixed(72.0))
                .height(Length::Fill)
                .push(
                    Container::new(
                        Svg::new(svg::Handle::from_path("src/bin/qrocks/RocksDB.svg"))
                            .width(Length::Fixed(60.0))
                            .height(Length::Fixed(60.0))
                    )
                    .padding(6)
                    .center_x()
                )
        )
        .style(theme::Container::Custom(Box::new(CustomContainer(Self::hex_to_color("#404040")))))
        .width(Length::Fixed(72.0))
        .height(Length::Fill);

        let files_panel = Container::new(
            Column::new()
                .spacing(10)
                .push(Text::new("Your Files").size(self.font_size))
                .push(
                    Container::new(
                        Scrollable::new(
                            self.files
                                .iter()
                                .fold(Column::new().spacing(5), |column, file| {
                                    column.push(
                                        Container::new(
                                            Row::new()
                                                .spacing(10)
                                                .push(Text::new(&file.name).size(self.font_size))
                                                .push(Text::new(&file.modified).size(self.font_size))
                                                .push(Text::new(format!("{}B", file.size)).size(self.font_size))
                                        )
                                        .width(Length::Fill)
                                        .style(theme::Container::Custom(Box::new(CustomContainer(
                                            Self::hex_to_color("#202020")
                                        ))))
                                        .padding(5)
                                    )
                                })
                        )
                    )
                    .height(Length::Fixed(350.0))
                    .width(Length::Fixed(700.0))
                    .padding(10)
                    .style(theme::Container::Custom(Box::new(BorderedContainer)))
                )
        )
        .height(Length::Fill)
        .width(Length::Fill);

        let main_content = Column::new()
            .spacing(20)
            .padding(20)
            .max_width(1200)
            .height(Length::Fill)
            .push(header)
            .push(files_panel);

        let content = Row::new()
            .push(side_panel)
            .push(main_content);

        Container::new(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
    }

    fn view_login_modal(&self) -> Element<Message> {
        let content = Column::new()
            .spacing(20)
            .padding(20)
            .push(Text::new("Login").size(self.font_size * 1.5))
            .push(
                TextInput::new(
                    "Username",
                    &self.login_state.username,
                )
                .size(self.font_size)
                .padding(8)
                .on_input(Message::UpdateUsername),
            )
            .push(
                TextInput::new(
                    "Password",
                    &self.login_state.password,
                )
                .size(self.font_size)
                .padding(8)
                .on_input(Message::UpdatePassword),
            )
            .push(
                Button::new(Text::new("Login").size(self.font_size))
                    .padding([4, 8])
                    .on_press(Message::AttemptLogin)
                    .style(theme::Button::Primary),
            );

        let modal_content = if let Some(error) = &self.login_state.error {
            content.push(
                Text::new(error)
                    .size(self.font_size)
                    .style(Color::from_rgb(0.8, 0.0, 0.0))
            )
        } else {
            content
        };

        Container::new(modal_content)
            .width(Length::Fixed(300.0))
            .padding(20)
            .style(theme::Container::Custom(Box::new(BorderedContainer)))
            .into()
    }

    fn hex_to_color(hex: &str) -> Color {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
        Color::from_rgb(r, g, b)
    }

    /*async fn cleanup(&mut self) {
        if self.nfs_client.is_some() {
            self.nfs_client = None;
            // Any additional cleanup needed
        }
    }*/
}

#[tokio::main]
async fn main() -> iced::Result {
    // Load settings from config file
    let mut settings = Config::default();
    settings
        .merge(ConfigFile::with_name("config/settings.toml"))
        .expect("Failed to load configuration");

    // Get log settings from configuration
    let base_level = settings
        .get::<String>("logging.level")
        .unwrap_or_else(|_| "debug".to_string());

    // Build filter with module directives
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            let mut filter = EnvFilter::new(&base_level);
            if let Ok(filters) = settings.get::<Vec<String>>("logging.module_filter") {
                for module_filter in filters {
                    filter = filter.add_directive(module_filter.parse().unwrap());
                }
            }
            filter
        });

    debug!("filter: {:?}", filter);

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(true)
        .with_line_number(true)
        .with_level(true)
        .compact()
        .init();

    let (mut data_room, _command) = DataRoom::new(());
    
    // Set up cleanup on ctrl+c
    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            //data_room.cleanup().await;
        }
    });

    DataRoom::run(Settings {
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
