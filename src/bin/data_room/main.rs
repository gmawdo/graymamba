use iced::{
    widget::{Column, Container, Text, Scrollable, Button, Row, TextInput},
    Element, Length, Application, Settings, Color, Alignment,
    theme::{self, Theme},
    Command,
    Border,
    Shadow,
    window,
    Size,
    keyboard::{self, Key},
    Subscription,
    widget::container,
    Font, Pixels,
    alignment::Horizontal,
};
use iced::widget::svg::Svg;
use iced::advanced::svg;
use std::error::Error;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use config::{Config, File as ConfigFile};
use std::sync::Arc;
use tokio::sync::Mutex;

use graymamba::nfsclient::{
    mount::{self, MountReply},
    null,
    readdirplus::{self, ReaddirplusReply},
    send_rpc_message,
    receive_rpc_reply,
};

use tokio::net::TcpStream;
use std::net::SocketAddr;

// State for login modal
#[derive(Debug, Default)]
struct LoginState {
    username: String,
    error: Option<String>,
    is_visible: bool,
}

#[derive(Debug, Clone)]
struct NfsSession {
    stream: Arc<Mutex<TcpStream>>,
    fs_handle: [u8; 16],
    dir_file_handles: Vec<([u8; 16], String, u64)>, // (handle, name, size)
}

#[derive(Debug)]
struct DataRoom {
    login_state: LoginState,
    authenticated_user: Option<String>,
    files: Vec<FileEntry>,
    error_message: Option<String>,
    font_size: f32,
    nfs_session: Option<NfsSession>,
    runtime_handle: tokio::runtime::Handle,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    size: u64,
    modified: String,
    file_id: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Message {
    ShowLogin,
    CloseLogin,
    UpdateUsername(String),
    AttemptLogin,
    Logout,
    LogoutComplete,
    RefreshFiles,
    FontSizeChanged(f32),
    Login,
    NfsConnected(Result<NfsSession, String>),
    NfsFilesLoaded(Result<(Vec<FileEntry>, Vec<([u8; 16], String, u64)>), String>),
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
                0.75,
                0.75,
                0.75,
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

#[derive(Debug)]
pub enum NfsError {
    NetworkError(String),
    ProtocolError(String),
    MountError(String),
    IoError(std::io::Error),
}

impl std::error::Error for NfsError {}

impl std::fmt::Display for NfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NfsError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            NfsError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            NfsError::MountError(msg) => write!(f, "Mount error: {}", msg),
            NfsError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl From<std::io::Error> for NfsError {
    fn from(err: std::io::Error) -> Self {
        NfsError::IoError(err)
    }
}

impl Application for DataRoom {
    type Message = Message;
    type Theme = Theme;
    type Executor = iced::executor::Default;
    type Flags = tokio::runtime::Handle;

    fn new(flags: Self::Flags) -> (Self, Command<Message>) {
        (
            DataRoom {
                login_state: LoginState::default(),
                authenticated_user: None,
                files: Vec::new(),
                error_message: None,
                font_size: 12.0,
                nfs_session: None,
                runtime_handle: flags,
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
                Command::none()
            }
            Message::CloseLogin => {
                self.login_state.is_visible = false;
                self.login_state.error = None;
                Command::none()
            }
            Message::UpdateUsername(username) => {
                self.login_state.username = username;
                Command::none()
            }
            Message::AttemptLogin => {
                let username = self.login_state.username.clone();
                let handle = self.runtime_handle.clone();
                
                Command::perform(
                    async move {
                        handle.spawn(async move {
                            DataRoom::connect_nfs(&username).await
                        }).await.unwrap()
                    },
                    |result| match result {
                        Ok(session) => Message::NfsConnected(Ok(session)),
                        Err(e) => Message::NfsConnected(Err(e.to_string()))
                    }
                )
            }
            Message::Logout => {
                if let Some(session) = &self.nfs_session {
                    let session = session.clone();
                    Command::perform(
                        async move {
                            if let Err(e) = session.disconnect().await {
                                debug!("Error during NFS disconnect: {}", e);
                            }
                            Ok::<(), Box<dyn Error>>(())
                        },
                        |_result: Result<(), Box<dyn Error>>| Message::LogoutComplete
                    )
                } else {
                    Command::perform(
                        async { Ok::<(), Box<dyn Error>>(()) },
                        |_: Result<(), Box<dyn Error>>| Message::LogoutComplete
                    )
                }
            }
            Message::LogoutComplete => {
                self.authenticated_user = None;
                self.nfs_session = None;
                self.files.clear();
                self.error_message = None;
                Command::none()
            }
            Message::RefreshFiles => {
                if let Some(session) = &self.nfs_session {
                    let fs_handle = session.fs_handle;
                    debug!("Starting refresh, fs_handle: {:02x?}", fs_handle);
                    let stream = session.stream.clone();

                    Command::perform(
                        async move {
                            debug!("Building readdirplus call");
                            let readdirplus_call = readdirplus::build_readdirplus_call(
                                4,
                                &fs_handle,
                                0,
                                0,
                                8192,
                                32768
                            );

                            let mut stream_guard = stream.lock().await;
                            debug!("Sending RPC message");
                            send_rpc_message(&mut *stream_guard, &readdirplus_call).await?;

                            debug!("Receiving RPC reply");
                            let reply = receive_rpc_reply(&mut *stream_guard).await?;
                            let readdir_reply = ReaddirplusReply::from_bytes(&reply)?;
                            if readdir_reply.status != 0 {
                                return Err("Failed to read directory".into());
                            }
                            debug!("Got readdir_reply with {} entries", readdir_reply.entries.len());

                            let mut files = Vec::new();
                            let mut handles = Vec::new();

                            for entry in readdir_reply.entries {
                                debug!("Processing entry: {} (id: {})", entry.name, entry.fileid);
                                if let (Some(attrs), Some(handle)) = (&entry.name_attributes, &entry.name_handle) {
                                    handles.push((*handle, entry.name.clone(), attrs.size));
                                    files.push(FileEntry {
                                        name: entry.name,
                                        size: attrs.size,
                                        modified: "".to_string(),
                                        file_id: entry.fileid,
                                    });
                                }
                            }
                            debug!("Processed {} files", files.len());

                            Ok((files, handles))
                        },
                        |result: Result<(Vec<FileEntry>, Vec<([u8; 16], String, u64)>), Box<dyn Error>>| {
                            match result {
                                Ok((files, handles)) => {
                                    debug!("NfsFilesLoaded: {} files", files.len());
                                    Message::NfsFilesLoaded(Ok((files, handles)))
                                }
                                Err(e) => {
                                    debug!("NfsFilesLoaded error: {}", e);
                                    Message::NfsFilesLoaded(Err(e.to_string()))
                                }
                            }
                        }
                    )
                } else {
                    debug!("RefreshFiles called with no active session");
                    Command::none()
                }
            }
            Message::FontSizeChanged(delta) => {
                self.font_size = (self.font_size + delta).max(8.0);
                Command::none()
            }
            Message::Login => {
                let username = self.login_state.username.clone();
                self.authenticated_user = Some(username);
                Command::none()
            }
            Message::NfsConnected(result) => {
                match result {
                    Ok(session) => {
                        self.nfs_session = Some(session);
                        self.authenticated_user = Some(self.login_state.username.clone());
                        self.login_state.is_visible = false;
                        Command::perform(
                            async { Ok::<(), Box<dyn Error>>(()) },
                            |_: Result<(), Box<dyn Error>>| Message::RefreshFiles
                        )
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                        Command::none()
                    }
                }
            }
            Message::NfsFilesLoaded(result) => {
                match result {
                    Ok((files, handles)) => {
                        debug!("Updating UI with {} files", files.len());
                        if let Some(session) = &mut self.nfs_session {
                            session.dir_file_handles = handles;
                        }
                        self.files = files;
                        Command::none()
                    }
                    Err(e) => {
                        debug!("Error loading files: {}", e);
                        self.error_message = Some(e);
                        Command::none()
                    }
                }
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let main_content = {
            let side_panel = Container::new(
                Column::new()
                    .width(Length::Fixed(72.0))
                    .height(Length::Fill)
                    .spacing(10)
                    .push(
                        Container::new(
                            Svg::new(svg::Handle::from_path("src/bin/qrocks/RocksDB.svg"))
                                .width(Length::Fixed(60.0))
                                .height(Length::Fixed(60.0))
                        )
                        .padding(6)
                        .center_x()
                    )
                    .push(
                        Container::new(
                            Button::new(Text::new("🔄 Refresh").size(self.font_size))
                                .padding([4, 8])
                                .on_press(Message::RefreshFiles)
                        )
                        .center_x()
                    )
                    .push(
                        Container::new(
                            if self.authenticated_user.is_none() {
                                Button::new(Text::new("🚀 Login").size(self.font_size))
                                    .padding([4, 8])
                                    .on_press(Message::ShowLogin)
                                    .style(theme::Button::Primary)
                            } else {
                                Button::new(Text::new("🚀 Logout").size(self.font_size))
                                    .padding([4, 8])
                                    .on_press(Message::Logout)
                                    .style(theme::Button::Secondary)
                            }
                        )
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
                                    //.push(Text::new(format!("Connected as: {}", 
                                    //    self.authenticated_user.as_ref().unwrap_or(&"Guest".to_string()))))
                                    .push(
                                        Row::new()
                                            .spacing(20)
                                            .push(
                                                {
                                                    let mut row = Row::new().spacing(20).padding(10);
                                                    for file in &self.files {
                                                        row = row.push(
                                                            Column::new()
                                                                .spacing(5)
                                                                .align_items(Alignment::Center)
                                                                .push(
                                                                    Text::new(Self::get_file_emoji(&file.name))
                                                                        .size(32.0)
                                                                )
                                                                .push(
                                                                    Text::new(&file.name)
                                                                        .size(self.font_size)
                                                                        .width(Length::Fixed(100.0))
                                                                        .horizontal_alignment(Horizontal::Center)
                                                                )
                                                        );
                                                    }
                                                    row
                                                }
                                            )
                                    )
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
                .push(files_panel);

            Row::new()
                .push(side_panel)
                .push(main_content)
        };

        if self.login_state.is_visible {
            // Stack the login modal on top of the main content
            Container::new(
                Column::new()
                    .push(
                        // Main content with blur effect
                        Container::new(main_content)
                            .style(theme::Container::Custom(Box::new(CustomContainer(Color::from_rgba(
                                0.0,
                                0.0,
                                0.0,
                                0.3, // More transparent to simulate blur
                            )))))
                    )
                    .push(
                        // Login modal
                        Container::new(self.view_login_modal())
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(theme::Container::Custom(Box::new(CustomContainer(Color::from_rgba(
                                0.0,
                                0.0,
                                0.0,
                                0.5,
                            )))))
                    )
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            Container::new(main_content)
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
    #[allow(dead_code)]
    fn view_main_content(&self) -> Element<Message> {
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
            .push(files_panel);

        Container::new(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
    }

    fn view_login_modal(&self) -> Element<Message> {
        let content = Column::new()
            .spacing(20)
            .padding(20)
            .push(
                Row::new()
                    .align_items(Alignment::Center)
                    .spacing(10)
                    .push(Text::new("Login").size(self.font_size * 1.5))
                    .push(
                        Container::new(
                            Button::new(Text::new("✕").size(self.font_size))
                                .on_press(Message::CloseLogin)
                                .style(theme::Button::Secondary)
                        )
                        .width(Length::Fill)
                        .align_x(Horizontal::Right)
                    )
            )
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
                Button::new(Text::new("Login").size(self.font_size))
                    .padding([4, 8])
                    .on_press(Message::AttemptLogin)
                    .style(theme::Button::Primary),
            );

        Container::new(
            Container::new(content)
                .width(Length::Fixed(300.0))
                .padding(20)
                .style(theme::Container::Custom(Box::new(BorderedContainer)))
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
    }

    fn hex_to_color(hex: &str) -> Color {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32 / 255.0;
        Color::from_rgb(r, g, b)
    }

    async fn connect_nfs(username: &str) -> Result<NfsSession, NfsError> {
        debug!("Starting NFS connection sequence");
        
        let mut settings = Config::default();
        settings
            .merge(ConfigFile::with_name("config/settings.toml"))
            .expect("Failed to load configuration");

        // Get NFS server address from config, with fallback
        let nfs_addr = settings
            .get_str("nfs.data_room_address")
            .unwrap_or_else(|_| "127.0.0.1:2049".to_string());
        debug!("===============nfs_addr({})", nfs_addr);

        let addr: SocketAddr = nfs_addr.parse()
            .map_err(|e: std::net::AddrParseError| NfsError::NetworkError(e.to_string()))?;
        
        let mut stream = TcpStream::connect(addr).await?;

        // NULL call
        let null_call = null::build_null_call(1);
        send_rpc_message(&mut stream, &null_call).await
            .map_err(|e| NfsError::ProtocolError(e.to_string()))?;
        receive_rpc_reply(&mut stream).await
            .map_err(|e| NfsError::ProtocolError(e.to_string()))?;

        // MOUNT call
        let mount_call = mount::build_mount_call(2, username);
        send_rpc_message(&mut stream, &mount_call).await
            .map_err(|e| NfsError::ProtocolError(e.to_string()))?;
        
        let reply = receive_rpc_reply(&mut stream).await
            .map_err(|e| NfsError::ProtocolError(e.to_string()))?;
        
        let mount_reply = MountReply::from_bytes(&reply)
            .map_err(|e| NfsError::ProtocolError(e.to_string()))?;

        if mount_reply.status != 0 {
            return Err(NfsError::MountError(
                format!("Mount failed with status: {}", mount_reply.status)
            ));
        }

        Ok(NfsSession {
            stream: Arc::new(Mutex::new(stream)),
            fs_handle: mount_reply.file_handle,
            dir_file_handles: Vec::new(),
        })
    }
    #[allow(dead_code)]
    async fn load_directory(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(session) = &mut self.nfs_session {
            let readdirplus_call = readdirplus::build_readdirplus_call(
                4,
                &session.fs_handle,
                0,
                0,
                8192,
                32768
            );

            let mut stream_guard = session.stream.lock().await;
            send_rpc_message(&mut *stream_guard, &readdirplus_call).await?;
            let reply = receive_rpc_reply(&mut *stream_guard).await?;
            let readdir_reply = ReaddirplusReply::from_bytes(&reply)?;

            if readdir_reply.status != 0 {
                return Err("Failed to read directory".into());
            }

            session.dir_file_handles.clear();
            self.files.clear();

            for entry in readdir_reply.entries {
                if let (Some(attrs), Some(handle)) = (&entry.name_attributes, &entry.name_handle) {
                    session.dir_file_handles.push((*handle, entry.name.clone(), attrs.size));
                    self.files.push(FileEntry {
                        name: entry.name,
                        size: attrs.size,
                        modified: "".to_string(), // TODO: Add timestamp conversion
                        file_id: entry.fileid,
                    });
                }
            }
        }
        Ok(())
    }

    fn get_file_emoji(filename: &str) -> &'static str {
        debug!("Getting file emoji for: {}", filename);
        if let Some(extension) = filename.split('.').last() {
            debug!("Extension: {}", extension);
            match extension.to_lowercase().as_str() {
                "txt" | "md" | "doc" | "docx" => "📝",
                "pdf" => "📕",
                "jpg" | "jpeg" | "png" | "gif" | "svg" => "🖼️",
                "mp3" | "wav" | "ogg" => "🎵",
                "mp4" | "mov" | "avi" => "🎬",
                "zip" | "rar" | "7z" => "🗜️",
                "exe" | "app" => "⚙️",
                _ => "📄",
            }
        } else {
            "📁" // For files without extensions or directories
        }
    }
}

impl NfsSession {
    async fn disconnect(&self) -> Result<(), Box<dyn Error>> {
        let mut stream = self.stream.lock().await;
        
        // Send NULL procedure call to gracefully end the session
        let null_call = null::build_null_call(1);
        send_rpc_message(&mut *stream, &null_call).await?;
        receive_rpc_reply(&mut *stream).await?;
        
        // Let the stream close naturally when dropped
        Ok(())
    }
}

#[tokio::main]
async fn main() -> iced::Result {
    let runtime_handle = tokio::runtime::Handle::current();
    
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

    let settings = Settings {
        window: window::Settings {
            size: Size {
                width: 1200.0,
                height: 800.0,
            },
            position: window::Position::Centered,
            ..window::Settings::default()
        },
        flags: runtime_handle,
        antialiasing: true,
        fonts: Default::default(),
        default_font: Font::default(),
        default_text_size: Pixels(16.0),
        id: None,
    };

    DataRoom::run(settings)
}
