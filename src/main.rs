use iced::futures::StreamExt;
use iced::widget::{button, center, column, text, Column};
use iced::window::settings::PlatformSpecific;
use iced::window::Position;
use iced::{Center, Element, Right, Settings, Size, Subscription};
use serde::{Deserialize, Serialize};
use uuid;

mod streaming_ollama;
use streaming_ollama::{subscribe_to_stream, Error as OllamaError, OllamaStreamProgress};

pub fn main() -> iced::Result {
    iced::application("Ollama GUI", OllamaGUI::update, OllamaGUI::view)
        .subscription(OllamaGUI::subscription)
        .settings(settings())
        .window(windows_settings())
        .run()
}

fn settings() -> Settings {
    Settings::default()
}

fn windows_settings() -> iced::window::Settings {
    let icon = iced::window::icon::from_file_data(
        include_bytes!("../images/logo.png"),
        Some(iced::advanced::graphics::image::image_rs::ImageFormat::Png),
    )
    .unwrap();
    iced::window::Settings {
        size: Size::new(1080.0, 720.0),
        position: Position::Centered,
        min_size: Some(Size::new(300.0, 100.0)),
        max_size: None,
        visible: true,
        resizable: true,
        decorations: true,
        transparent: false,
        level: iced::window::Level::Normal,
        icon: Some(icon),
        platform_specific: PlatformSpecific::default(),
        exit_on_close_request: true,
    }
}

#[derive(Debug)]
struct OllamaGUI {
    responses: Vec<Ollama>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Add,
    Request(uuid::Uuid),
    RequestProgressed((uuid::Uuid, Result<OllamaStreamProgress, OllamaError>)),
}

impl OllamaGUI {
    fn new() -> Self {
        Self {
            responses: vec![Ollama::new()],
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Add => {
                self.responses.push(Ollama::new());
            }
            Message::Request(id) => {
                if let Some(request) = self.responses.iter_mut().find(|ollama| ollama.uuid == id) {
                    request.start();
                }
            }
            Message::RequestProgressed((id, progress)) => {
                if let Some(request) = self.responses.iter_mut().find(|ollama| ollama.uuid == id) {
                    request.progress(progress);
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(self.responses.iter().map(Ollama::subscription))
    }

    fn view(&self) -> Element<Message> {
        let responses = Column::with_children(self.responses.iter().map(Ollama::view))
            .push(
                button("Add another stream")
                    .on_press(Message::Add)
                    .padding(10),
            )
            .spacing(20)
            .align_x(Right);

        center(responses).padding(20).into()
    }
}

impl Default for OllamaGUI {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
enum OllamaStreamState {
    Idle,
    Streaming { output: String },
    Finished { output: String, context: Vec<u64> },
    Errored,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatHistory {
    display_name: String,
    uuid: String,
    context: Vec<u64>,
    model: String,
    chat: Vec<ChatEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatEntry {
    prompt: String,
    response: String,
}

#[derive(Debug)]
struct Ollama {
    uuid: uuid::Uuid,
    display_name: String,
    state: OllamaStreamState,
    prompt: String,
    model: String,
    context: Option<Vec<u64>>,
}

impl Ollama {
    pub fn new() -> Self {
        let uuid = uuid::Uuid::new_v4();
        Self {
            uuid,
            display_name: format!("Chat-{}", uuid),
            state: OllamaStreamState::Idle,
            prompt: "prompt".to_string(),
            model: "phi4".to_string(),
            context: None,
        }
    }

    pub fn start(&mut self) {
        match self.state {
            OllamaStreamState::Idle
            | OllamaStreamState::Finished { .. }
            | OllamaStreamState::Errored => {
                self.state = OllamaStreamState::Streaming {
                    output: String::new(),
                };
            }
            OllamaStreamState::Streaming { .. } => {}
        }
    }

    pub fn progress(&mut self, new_progress: Result<OllamaStreamProgress, OllamaError>) {
        if let OllamaStreamState::Streaming { ref mut output } = self.state {
            match new_progress {
                Ok(OllamaStreamProgress::Streaming { token }) => {
                    output.push_str(&token);
                }
                Ok(OllamaStreamProgress::Finished { context }) => {
                    let final_output = if let OllamaStreamState::Streaming { output } =
                        std::mem::replace(&mut self.state, OllamaStreamState::Idle)
                    {
                        output
                    } else {
                        String::new()
                    };

                    self.context = Some(context.clone());
                    self.state = OllamaStreamState::Finished {
                        output: final_output.clone(),
                        context: context.clone(),
                    };

                    // Save as JSON
                    let chat_entry = ChatEntry {
                        prompt: self.prompt.clone(),
                        response: final_output,
                    };

                    let file_path = format!("./chats/{}.json", self.uuid);
                    if let Err(e) = std::fs::create_dir_all("./chats") {
                        eprintln!("Error creating chats directory: {}", e);
                    }

                    let mut chat_history = match std::fs::File::open(&file_path) {
                        Ok(file) => serde_json::from_reader(file).unwrap_or_else(|_| ChatHistory {
                            display_name: self.display_name.clone(),
                            uuid: self.uuid.to_string(),
                            context: vec![],
                            model: self.model.clone(),
                            chat: vec![],
                        }),
                        Err(_) => ChatHistory {
                            display_name: self.display_name.clone(),
                            uuid: self.uuid.to_string(),
                            context: vec![],
                            model: self.model.clone(),
                            chat: vec![],
                        },
                    };

                    chat_history.context = context;
                    chat_history.chat.push(chat_entry);

                    if let Ok(file) = std::fs::File::create(&file_path) {
                        if let Err(e) = serde_json::to_writer_pretty(file, &chat_history) {
                            eprintln!("Error writing JSON to file {}: {}", file_path, e);
                        }
                    } else {
                        eprintln!("Error creating file: {}", file_path);
                    }
                }
                Err(_error) => {
                    self.state = OllamaStreamState::Errored;
                }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self.state {
            OllamaStreamState::Streaming { .. } => subscribe_to_stream(
                self.uuid,
                "http://localhost:11434/api/generate",
                &self.prompt,
                &self.model,
            )
            .map(Message::RequestProgressed),
            _ => Subscription::none(),
        }
    }

    pub fn view(&self) -> Element<Message> {
        let control: Element<_> = match &self.state {
            OllamaStreamState::Idle => button("Start streaming from Ollama!")
                .on_press(Message::Request(self.uuid))
                .into(),
            OllamaStreamState::Streaming { output } => column!["Streaming output:", text(output)]
                .spacing(10)
                .align_x(Center)
                .into(),
            OllamaStreamState::Finished { output, context } => column![
                "Streaming finished!",
                "Final output:",
                text(output),
                "Context:",
                text(format!("{:?}", context)),
                button("Start again").on_press(Message::Request(self.uuid))
            ]
            .spacing(10)
            .align_x(Center)
            .into(),
            OllamaStreamState::Errored => column![
                "Something went wrong :(",
                button("Try again").on_press(Message::Request(self.uuid))
            ]
            .spacing(10)
            .align_x(Center)
            .into(),
        };

        Column::new()
            .spacing(10)
            .padding(10)
            .align_x(Center)
            .push(control)
            .into()
    }
}
