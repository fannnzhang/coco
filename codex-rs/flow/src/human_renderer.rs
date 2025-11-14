use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::io::{self};
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use codex_exec::exec_events::AgentMessageItem;
use codex_exec::exec_events::CommandExecutionItem;
use codex_exec::exec_events::CommandExecutionStatus;
use codex_exec::exec_events::ErrorItem;
use codex_exec::exec_events::FileChangeItem;
use codex_exec::exec_events::ItemCompletedEvent;
use codex_exec::exec_events::ItemStartedEvent;
use codex_exec::exec_events::ItemUpdatedEvent;
use codex_exec::exec_events::McpToolCallItem;
use codex_exec::exec_events::McpToolCallStatus;
use codex_exec::exec_events::PatchApplyStatus;
use codex_exec::exec_events::PatchChangeKind;
use codex_exec::exec_events::ThreadErrorEvent;
use codex_exec::exec_events::ThreadEvent;
use codex_exec::exec_events::ThreadItemDetails;
use codex_exec::exec_events::ThreadStartedEvent;
use codex_exec::exec_events::TodoListItem;
use codex_exec::exec_events::TurnCompletedEvent;
use codex_exec::exec_events::TurnFailedEvent;
use codex_exec::exec_events::Usage;
use codex_exec::exec_events::WebSearchItem;
use codex_protocol::num_format::format_with_separators;
use owo_colors::OwoColorize;
use owo_colors::Style;
use serde::Serialize;
use serde_json::Value as JsonValue;
use supports_color::Stream;

const MAX_OUTPUT_LINES_FOR_TOOL_CALL: usize = 20;

pub struct HumanEventRenderer {
    styles: Styles,
    command_outputs: HashMap<String, String>,
    output: OutputSink,
}

impl Default for HumanEventRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl HumanEventRenderer {
    pub fn new() -> Self {
        Self::with_output(OutputSink::stdout_only())
    }

    pub fn with_log_path(path: &Path) -> Result<Self> {
        let output = OutputSink::with_log_file(path)
            .with_context(|| format!("failed to create human output log {}", path.display()))?;
        Ok(Self::with_output(output))
    }

    fn with_output(output: OutputSink) -> Self {
        let with_ansi = supports_color::on_cached(Stream::Stdout).is_some();
        Self {
            styles: Styles::new(with_ansi),
            command_outputs: HashMap::new(),
            output,
        }
    }

    pub fn render_event(&mut self, event: &ThreadEvent) {
        match event {
            ThreadEvent::ThreadStarted(ev) => self.render_thread_started(ev),
            ThreadEvent::TurnStarted(_) => {}
            ThreadEvent::TurnCompleted(ev) => self.render_turn_completed(ev),
            ThreadEvent::TurnFailed(ev) => self.render_turn_failed(ev),
            ThreadEvent::ItemStarted(ev) => self.render_item_started(ev),
            ThreadEvent::ItemUpdated(ev) => self.render_item_updated(ev),
            ThreadEvent::ItemCompleted(ev) => self.render_item_completed(ev),
            ThreadEvent::Error(err) => self.render_stream_error(err),
        }
        self.output.log_event_separator();
    }

    pub fn log_plain_line(&mut self, text: &str) {
        if text.is_empty() {
            self.newline();
        } else {
            self.output.writeln(text);
        }
    }

    fn write_line(&mut self, text: impl Display) {
        let rendered = text.to_string();
        self.output.writeln(&rendered);
    }

    fn write_raw(&mut self, text: &str) {
        self.output.write(text);
    }

    fn newline(&mut self) {
        self.output.newline();
    }

    fn flush_output(&mut self) {
        self.output.flush();
    }

    fn render_thread_started(&mut self, ev: &ThreadStartedEvent) {
        self.write_line(format!(
            "{} {}",
            "codex session"
                .style(self.styles.magenta)
                .style(self.styles.bold),
            ev.thread_id.style(self.styles.dimmed)
        ));
        self.newline();
    }

    fn render_turn_completed(&mut self, ev: &TurnCompletedEvent) {
        let usage = &ev.usage;
        let totals = TurnTotals::from_usage(usage);
        self.write_line(format!(
            "{}\n{total} total (in {input} · cached {cached} · out {output})",
            "tokens used"
                .style(self.styles.magenta)
                .style(self.styles.italic),
            total = totals.total,
            input = totals.input,
            cached = totals.cached,
            output = totals.output,
        ));
    }

    fn render_turn_failed(&mut self, ev: &TurnFailedEvent) {
        self.write_line(format!(
            "{} {}",
            "error:".style(self.styles.red).style(self.styles.bold),
            ev.error.message.trim()
        ));
    }

    fn render_item_started(&mut self, ev: &ItemStartedEvent) {
        match &ev.item.details {
            ThreadItemDetails::CommandExecution(cmd) => self.render_command_start(&ev.item.id, cmd),
            ThreadItemDetails::TodoList(list) => self.render_plan_update(list),
            ThreadItemDetails::McpToolCall(call) => self.render_mcp_tool_call_begin(call),
            _ => {}
        }
    }

    fn render_item_updated(&mut self, ev: &ItemUpdatedEvent) {
        match &ev.item.details {
            ThreadItemDetails::CommandExecution(cmd) => {
                self.render_command_delta(&ev.item.id, &cmd.aggregated_output);
            }
            ThreadItemDetails::TodoList(list) => self.render_plan_update(list),
            _ => {}
        }
    }

    fn render_item_completed(&mut self, ev: &ItemCompletedEvent) {
        match &ev.item.details {
            ThreadItemDetails::AgentMessage(msg) => self.render_agent_message(msg),
            ThreadItemDetails::Reasoning(reason) => self.render_reasoning(reason),
            ThreadItemDetails::CommandExecution(cmd) => {
                self.render_command_delta(&ev.item.id, &cmd.aggregated_output);
                self.render_command_completion(&ev.item.id, cmd);
            }
            ThreadItemDetails::FileChange(change) => self.render_file_change(change),
            ThreadItemDetails::McpToolCall(call) => self.render_mcp_tool_call_end(call),
            ThreadItemDetails::WebSearch(search) => self.render_web_search(search),
            ThreadItemDetails::TodoList(list) => self.render_plan_update(list),
            ThreadItemDetails::Error(err) => self.render_inline_error(err),
        }
    }

    fn render_agent_message(&mut self, msg: &AgentMessageItem) {
        let text = msg.text.trim_end();
        if text.is_empty() {
            return;
        }
        self.write_line(format!(
            "{}\n{text}",
            "codex".style(self.styles.magenta).style(self.styles.italic)
        ));
    }

    fn render_reasoning(&mut self, reason: &codex_exec::exec_events::ReasoningItem) {
        let text = reason.text.trim_end();
        if text.is_empty() {
            return;
        }
        self.write_line(format!(
            "{}\n{text}",
            "thinking"
                .style(self.styles.magenta)
                .style(self.styles.italic)
        ));
    }

    fn render_command_start(&mut self, item_id: &str, cmd: &CommandExecutionItem) {
        self.write_line(format!(
            "{}\n{}",
            "exec".style(self.styles.magenta).style(self.styles.italic),
            cmd.command.style(self.styles.bold)
        ));
        self.command_outputs
            .insert(item_id.to_string(), cmd.aggregated_output.clone());
    }

    fn render_command_completion(&mut self, item_id: &str, cmd: &CommandExecutionItem) {
        let exit_description = match cmd.exit_code {
            Some(code) => format!("exit {code}"),
            None => "exit unknown".to_string(),
        };
        let status = match cmd.status {
            CommandExecutionStatus::Completed => ("succeeded", self.styles.green),
            CommandExecutionStatus::Failed => ("failed", self.styles.red),
            CommandExecutionStatus::InProgress => ("in-progress", self.styles.yellow),
        };
        self.write_line(
            format!(
                "{command} {state} ({exit_description})",
                command = cmd.command,
                state = status.0
            )
            .style(status.1),
        );
        self.command_outputs.remove(item_id);
    }

    fn render_command_delta(&mut self, item_id: &str, aggregated_output: &str) {
        let previous = self
            .command_outputs
            .get(item_id)
            .cloned()
            .unwrap_or_default();
        if aggregated_output.len() >= previous.len() {
            let delta = &aggregated_output[previous.len()..];
            if !delta.is_empty() {
                self.write_raw(delta);
                if !delta.ends_with('\n') {
                    self.newline();
                }
                self.flush_output();
            }
        } else if !aggregated_output.is_empty() {
            self.write_raw(aggregated_output);
            if !aggregated_output.ends_with('\n') {
                self.newline();
            }
            self.flush_output();
        }
        self.command_outputs
            .insert(item_id.to_string(), aggregated_output.to_string());
    }

    fn render_file_change(&mut self, change: &FileChangeItem) {
        let status = change.status.clone();
        let status_style = match status {
            PatchApplyStatus::Completed => self.styles.green,
            PatchApplyStatus::Failed => self.styles.red,
        };
        let status_text = format!("{status:?}").to_lowercase();
        self.write_line(format!(
            "{} {}",
            "file update"
                .style(self.styles.magenta)
                .style(self.styles.italic),
            status_text.style(status_style)
        ));
        for file_change in &change.changes {
            let (marker, marker_style) = match file_change.kind {
                PatchChangeKind::Add => ("A", self.styles.green),
                PatchChangeKind::Delete => ("D", self.styles.red),
                PatchChangeKind::Update => ("M", self.styles.yellow),
            };
            self.write_line(format!(
                "  {} {}",
                marker.style(marker_style),
                file_change.path.style(self.styles.bold),
            ));
        }
    }

    fn render_mcp_tool_call_begin(&mut self, call: &McpToolCallItem) {
        self.write_line(format!(
            "{} {}",
            "tool".style(self.styles.magenta),
            format_mcp_invocation(&call.server, &call.tool, &call.arguments)
                .style(self.styles.bold)
        ));
    }

    fn render_mcp_tool_call_end(&mut self, call: &McpToolCallItem) {
        let invocation = format_mcp_invocation(&call.server, &call.tool, &call.arguments);
        let (status_label, status_style) = match call.status {
            McpToolCallStatus::Completed => ("success", self.styles.green),
            McpToolCallStatus::Failed => ("failed", self.styles.red),
            McpToolCallStatus::InProgress => ("in-progress", self.styles.yellow),
        };
        self.write_line(format!("{invocation} {status_label}").style(status_style));
        if let Some(result) = &call.result {
            self.render_tool_payload(result);
        }
        if let Some(error) = &call.error {
            self.write_line(error.message.style(self.styles.red));
        }
    }

    fn render_tool_payload<T: Serialize>(&mut self, payload: &T) {
        match serde_json::to_string_pretty(payload) {
            Ok(pretty) => {
                for line in pretty.lines().take(MAX_OUTPUT_LINES_FOR_TOOL_CALL) {
                    self.write_line(line.style(self.styles.dimmed));
                }
            }
            Err(err) => self
                .write_line(format!("Failed to print tool output: {err}").style(self.styles.red)),
        }
    }

    fn render_plan_update(&mut self, list: &TodoListItem) {
        self.write_line("Plan update".style(self.styles.magenta));
        for item in &list.items {
            if item.completed {
                self.write_line(format!(
                    "  {} {}",
                    "✓".style(self.styles.green),
                    item.text.style(self.styles.bold)
                ));
            } else {
                self.write_line(format!(
                    "  {} {}",
                    "•".style(self.styles.dimmed),
                    item.text.style(self.styles.dimmed)
                ));
            }
        }
    }

    fn render_web_search(&mut self, search: &WebSearchItem) {
        let query = &search.query;
        self.write_line(format!("🌐 Searched: {query}").style(self.styles.dimmed));
    }

    fn render_inline_error(&mut self, err: &ErrorItem) {
        self.write_line(format!(
            "{} {}",
            "warning:".style(self.styles.yellow).style(self.styles.bold),
            err.message.trim()
        ));
    }

    fn render_stream_error(&mut self, err: &ThreadErrorEvent) {
        self.write_line(format!(
            "{} {}",
            "stream error:"
                .style(self.styles.red)
                .style(self.styles.bold),
            err.message.trim()
        ));
    }
}

struct TurnTotals {
    total: String,
    input: String,
    cached: String,
    output: String,
}

impl TurnTotals {
    fn from_usage(usage: &Usage) -> Self {
        let total_value = usage.input_tokens + usage.cached_input_tokens + usage.output_tokens;
        Self {
            total: format_with_separators(total_value),
            input: format_with_separators(usage.input_tokens),
            cached: format_with_separators(usage.cached_input_tokens),
            output: format_with_separators(usage.output_tokens),
        }
    }
}

struct OutputSink {
    stdout: io::Stdout,
    file: Option<BufWriter<File>>,
}

impl OutputSink {
    fn stdout_only() -> Self {
        Self {
            stdout: io::stdout(),
            file: None,
        }
    }

    fn with_log_file(path: &Path) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self {
            stdout: io::stdout(),
            file: Some(BufWriter::new(file)),
        })
    }

    fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let _ = self.stdout.write_all(text.as_bytes());
        if let Some(file) = &mut self.file {
            let plain = strip_ansi_codes(text);
            let _ = file.write_all(plain.as_ref().as_bytes());
        }
    }

    fn writeln(&mut self, text: &str) {
        self.write(text);
        self.write_newline();
    }

    fn newline(&mut self) {
        self.write_newline();
    }

    fn write_newline(&mut self) {
        let _ = self.stdout.write_all(b"\n");
        if let Some(file) = &mut self.file {
            let _ = file.write_all(b"\n");
        }
    }

    fn flush(&mut self) {
        let _ = self.stdout.flush();
        if let Some(file) = &mut self.file {
            let _ = file.flush();
        }
    }

    fn log_event_separator(&mut self) {
        if let Some(file) = &mut self.file {
            let _ = file.write_all(b"\n");
        }
    }
}

fn strip_ansi_codes(text: &str) -> Cow<'_, str> {
    if !text.contains('\x1b') {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some(next) = chars.peek().copied() {
                match next {
                    '[' => {
                        chars.next();
                        for c in chars.by_ref() {
                            if ('@'..='~').contains(&c) {
                                break;
                            }
                        }
                        continue;
                    }
                    ']' => {
                        chars.next();
                        while let Some(c) = chars.next() {
                            if c == '\x07' {
                                break;
                            }
                            if c == '\x1b'
                                && let Some('\\') = chars.peek().copied()
                            {
                                chars.next();
                                break;
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            // Unrecognized escape, skip it.
            continue;
        }
        output.push(ch);
    }
    Cow::Owned(output)
}

struct Styles {
    bold: Style,
    italic: Style,
    dimmed: Style,
    magenta: Style,
    red: Style,
    green: Style,
    yellow: Style,
}

impl Styles {
    fn new(with_ansi: bool) -> Self {
        if with_ansi {
            Self {
                bold: Style::new().bold(),
                italic: Style::new().italic(),
                dimmed: Style::new().dimmed(),
                magenta: Style::new().magenta(),
                red: Style::new().red(),
                green: Style::new().green(),
                yellow: Style::new().yellow(),
            }
        } else {
            let style = Style::new();
            Self {
                bold: style,
                italic: style,
                dimmed: style,
                magenta: style,
                red: style,
                green: style,
                yellow: style,
            }
        }
    }
}

fn format_mcp_invocation(server: &str, tool: &str, arguments: &JsonValue) -> String {
    let fq_tool_name = format!("{server}.{tool}");
    let args_str = match arguments {
        JsonValue::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    };
    if args_str.is_empty() || args_str == "null" {
        format!("{fq_tool_name}()")
    } else {
        format!("{fq_tool_name}({args_str})")
    }
}
