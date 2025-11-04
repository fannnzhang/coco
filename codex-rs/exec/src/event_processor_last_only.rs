use std::path::PathBuf;

use codex_core::config::Config;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::SessionConfiguredEvent;
use codex_core::protocol::TaskCompleteEvent;

use crate::event_processor::CodexStatus;
use crate::event_processor::EventProcessor;
use crate::event_processor::handle_last_message;

pub(crate) struct EventProcessorLastOnly {
    last_message_path: Option<PathBuf>,
    final_message: Option<String>,
}

impl EventProcessorLastOnly {
    pub(crate) fn new(last_message_path: Option<PathBuf>) -> Self {
        Self {
            last_message_path,
            final_message: None,
        }
    }
}

impl EventProcessor for EventProcessorLastOnly {
    fn print_config_summary(&mut self, _: &Config, _: &str, _: &SessionConfiguredEvent) {}

    fn process_event(&mut self, event: Event) -> CodexStatus {
        match event.msg {
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                if let Some(path) = self.last_message_path.as_deref() {
                    handle_last_message(last_agent_message.as_deref(), path);
                }
                self.final_message = last_agent_message;
                CodexStatus::InitiateShutdown
            }
            EventMsg::ShutdownComplete => CodexStatus::Shutdown,
            _ => CodexStatus::Running,
        }
    }

    fn print_final_output(&mut self) {
        #[allow(clippy::print_stdout)]
        if let Some(message) = &self.final_message {
            if message.ends_with('\n') {
                print!("{message}");
            } else {
                println!("{message}");
            }
        }
    }
}
