mod download;

use iced::widget::{button, center, column, text, Column};
use iced::{Center, Element, Right, Subscription};

pub fn main() -> iced::Result {
    iced::application("Ollama Stream - Iced", OllamaGUI::update, OllamaGUI::view)
        .subscription(OllamaGUI::subscription)
        .run()
}

#[derive(Debug)]
struct OllamaGUI {
    downloads: Vec<Ollama>,
    last_id: usize,
}

#[derive(Debug, Clone)]
pub enum Message {
    Add,
    Download(usize),
    DownloadProgressed(
        (
            usize,
            Result<download::OllamaStreamProgress, download::Error>,
        ),
    ),
}

impl OllamaGUI {
    fn new() -> Self {
        Self {
            downloads: vec![Ollama::new(0)],
            last_id: 0,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Add => {
                self.last_id += 1;
                self.downloads.push(Ollama::new(self.last_id));
            }
            Message::Download(id) => {
                if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
                    download.start();
                }
            }
            Message::DownloadProgressed((id, progress)) => {
                if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
                    download.progress(progress);
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(self.downloads.iter().map(Ollama::subscription))
    }

    fn view(&self) -> Element<Message> {
        let downloads = Column::with_children(self.downloads.iter().map(Ollama::view))
            .push(
                button("Add another stream")
                    .on_press(Message::Add)
                    .padding(10),
            )
            .spacing(20)
            .align_x(Right);

        center(downloads).padding(20).into()
    }
}

impl Default for OllamaGUI {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct Ollama {
    id: usize,
    state: OllamaStreamState,
}

#[derive(Debug)]
enum OllamaStreamState {
    Idle,
    Streaming { output: String },
    Finished { output: String },
    Errored,
}

impl Ollama {
    pub fn new(id: usize) -> Self {
        Ollama {
            id,
            state: OllamaStreamState::Idle,
        }
    }

    pub fn start(&mut self) {
        // Transition into a streaming state when starting
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

    pub fn progress(
        &mut self,
        new_progress: Result<download::OllamaStreamProgress, download::Error>,
    ) {
        if let OllamaStreamState::Streaming { ref mut output } = self.state {
            match new_progress {
                Ok(download::OllamaStreamProgress::Streaming { token }) => {
                    output.push_str(&token);
                }
                Ok(download::OllamaStreamProgress::Finished) => {
                    // When finished, preserve the final output
                    let final_output = if let OllamaStreamState::Streaming { output } =
                        std::mem::replace(&mut self.state, OllamaStreamState::Idle)
                    {
                        output
                    } else {
                        String::new()
                    };
                    self.state = OllamaStreamState::Finished {
                        output: final_output,
                    };
                }
                Err(_error) => {
                    self.state = OllamaStreamState::Errored;
                }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self.state {
            OllamaStreamState::Streaming { .. } => {
                // Replace the URL with your actual Ollama streaming endpoint.
                download::subscribe_to_stream(self.id, "http://localhost:11434/api/generate")
                    .map(Message::DownloadProgressed)
            }
            _ => Subscription::none(),
        }
    }

    pub fn view(&self) -> Element<Message> {
        let control: Element<_> = match &self.state {
            OllamaStreamState::Idle => button("Start streaming from Ollama!")
                .on_press(Message::Download(self.id))
                .into(),
            OllamaStreamState::Streaming { output } => column!["Streaming output:", text(output)]
                .spacing(10)
                .align_x(Center)
                .into(),
            OllamaStreamState::Finished { output } => column![
                "Streaming finished!",
                "Final output:",
                text(output),
                button("Start again").on_press(Message::Download(self.id))
            ]
            .spacing(10)
            .align_x(Center)
            .into(),
            OllamaStreamState::Errored => column![
                "Something went wrong :(",
                button("Try again").on_press(Message::Download(self.id))
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
