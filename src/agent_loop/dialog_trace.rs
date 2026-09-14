use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::agent_loop::DialogDirection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogSource {
    AgentLoop,
    SessionAutoname,
    SessionCompression,
    RiskEvaluator,
    OpenAIResponses,
    ShellExecute,
    Subagent,
    GenericTool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogEvent {
    pub trace_id: String,
    pub source: DialogSource,
    pub sequence: u64,
    pub agent: String,
    pub configured_model: String,
    pub wire_model: Option<String>,
    pub pid: u32,
    pub turn: usize,
    pub max_turns: usize,
    pub direction: DialogDirection,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogOutputDestination {
    Terminal,
    Stderr,
}

pub fn dialog_output_destination() -> DialogOutputDestination {
    if let Ok(val) = std::env::var("AICHAT_DIALOG_OUTPUT") {
        if val.eq_ignore_ascii_case("stderr") {
            return DialogOutputDestination::Stderr;
        }
    }
    DialogOutputDestination::Terminal
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct DialogTraceSink {
    tx: UnboundedSender<DialogEvent>,
    next_sequence: AtomicU64,
}

impl DialogTraceSink {
    pub fn new() -> (Arc<Self>, UnboundedReceiver<DialogEvent>) {
        let (tx, rx) = unbounded_channel();
        let sink = Arc::new(Self {
            tx,
            next_sequence: AtomicU64::new(1),
        });
        (sink, rx)
    }

    pub fn emit(&self, mut event: DialogEvent) {
        event.sequence = self.next_sequence.fetch_add(1, Ordering::SeqCst);
        if let Err(e) = self.tx.send(event) {
            debug!("DialogTraceSink: receiver dropped, cannot deliver event: {e}");
        }
    }

    #[allow(dead_code)]
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence.load(Ordering::SeqCst)
    }
}

pub fn render_dialog_event(event: &DialogEvent) {
    if std::env::var("AICHAT_DIALOG_RELAY").as_deref() == Ok("stderr") {
        if let Some(frame) = format_relay_frame(event) {
            use std::io::Write;
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(frame.as_bytes());
            let _ = stderr.flush();
            return;
        }
    }
    let block = crate::agent_loop::format_dialog_event(event);
    crate::agent_loop::emit_dialog_block_raw(&block);
}

pub const RELAY_FRAME_PREFIX: &str = "__AICHAT_DIALOG_EVENT__";

/// Format a DialogEvent as a length-delimited JSON frame on stderr:
/// `__AICHAT_DIALOG_EVENT__ <length>\n<json>\n`
pub fn format_relay_frame(event: &DialogEvent) -> Option<String> {
    let json = serde_json::to_string(event).ok()?;
    Some(format!("{} {}\n{}\n", RELAY_FRAME_PREFIX, json.len(), json))
}

/// Parse all length-delimited JSON relay frames out of `input`.
/// Returns the list of parsed `DialogEvent` objects and the remaining raw stderr text.
pub fn parse_relay_frames(input: &str) -> (Vec<DialogEvent>, String) {
    let mut events = Vec::new();
    let mut remaining = String::new();
    let mut rest = input;

    while let Some(pos) = rest.find(RELAY_FRAME_PREFIX) {
        remaining.push_str(&rest[..pos]);
        let after_prefix = &rest[pos + RELAY_FRAME_PREFIX.len()..];
        let after_prefix_trimmed = after_prefix.trim_start();
        if let Some(newline_pos) = after_prefix_trimmed.find('\n') {
            let len_str = after_prefix_trimmed[..newline_pos].trim();
            if let Ok(byte_len) = len_str.parse::<usize>() {
                let payload_start = newline_pos + 1;
                if after_prefix_trimmed.len() >= payload_start + byte_len {
                    let json_str = &after_prefix_trimmed[payload_start..payload_start + byte_len];
                    if let Ok(event) = serde_json::from_str::<DialogEvent>(json_str) {
                        events.push(event);
                        let mut next_pos = payload_start + byte_len;
                        if after_prefix_trimmed[next_pos..].starts_with('\n') {
                            next_pos += 1;
                        }
                        let consumed_in_after_prefix = (after_prefix.len() - after_prefix_trimmed.len()) + next_pos;
                        rest = &after_prefix[consumed_in_after_prefix..];
                        continue;
                    }
                }
            }
        }
        remaining.push_str(RELAY_FRAME_PREFIX);
        rest = &rest[pos + RELAY_FRAME_PREFIX.len()..];
    }
    remaining.push_str(rest);
    (events, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_loop::DialogDirection;

    #[test]
    fn test_relay_framing_roundtrip() {
        let event = DialogEvent {
            sequence: 42,
            trace_id: "test-trace".into(),
            agent: "coder".into(),
            configured_model: "claude-3-7-sonnet".into(),
            wire_model: Some("claude-3-7-sonnet-20250219".into()),
            pid: 12345,
            turn: 2,
            max_turns: 15,
            direction: DialogDirection::Request,
            source: DialogSource::AgentLoop,
            content: "Please refactor this code.".into(),
        };

        let frame = format_relay_frame(&event).expect("format frame");
        assert!(frame.starts_with(RELAY_FRAME_PREFIX));

        let mixed_stderr = format!("some build warning\n{frame}another diagnostic line\n");
        let (events, remaining) = parse_relay_frames(&mixed_stderr);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].trace_id, "test-trace");
        assert_eq!(events[0].agent, "coder");
        assert_eq!(events[0].configured_model, "claude-3-7-sonnet");
        assert_eq!(events[0].wire_model.as_deref(), Some("claude-3-7-sonnet-20250219"));
        assert_eq!(events[0].content, "Please refactor this code.");

        assert_eq!(remaining, "some build warning\nanother diagnostic line\n");
    }

    #[test]
    fn test_dialog_output_destination_override() {
        let prev = std::env::var("AICHAT_DIALOG_OUTPUT").ok();
        unsafe {
            std::env::set_var("AICHAT_DIALOG_OUTPUT", "stderr");
            assert_eq!(dialog_output_destination(), DialogOutputDestination::Stderr);
            std::env::set_var("AICHAT_DIALOG_OUTPUT", "tty");
            assert_eq!(dialog_output_destination(), DialogOutputDestination::Terminal);
            match prev {
                Some(v) => std::env::set_var("AICHAT_DIALOG_OUTPUT", v),
                None => std::env::remove_var("AICHAT_DIALOG_OUTPUT"),
            }
        }
    }

    #[test]
    fn test_sink_sequence_and_sources() {
        let (sink, mut rx) = DialogTraceSink::new();
        assert_eq!(sink.next_sequence(), 1);

        let event1 = DialogEvent {
            sequence: 0,
            trace_id: "t1".into(),
            source: DialogSource::SessionAutoname,
            agent: "autoname".into(),
            configured_model: "gpt-4o-mini".into(),
            wire_model: None,
            pid: 100,
            turn: 1,
            max_turns: 1,
            direction: DialogDirection::Request,
            content: "Name this session".into(),
        };
        sink.emit(event1);
        assert_eq!(sink.next_sequence(), 2);

        let event2 = DialogEvent {
            sequence: 0,
            trace_id: "t2".into(),
            source: DialogSource::SessionCompression,
            agent: "compress".into(),
            configured_model: "gpt-4o".into(),
            wire_model: None,
            pid: 100,
            turn: 1,
            max_turns: 1,
            direction: DialogDirection::Request,
            content: "Summarize this session".into(),
        };
        sink.emit(event2);
        assert_eq!(sink.next_sequence(), 3);

        let event3 = DialogEvent {
            sequence: 0,
            trace_id: "t3".into(),
            source: DialogSource::ShellExecute,
            agent: "shell".into(),
            configured_model: "claude-3-5-sonnet".into(),
            wire_model: None,
            pid: 100,
            turn: 1,
            max_turns: 1,
            direction: DialogDirection::Request,
            content: "echo hello".into(),
        };
        sink.emit(event3);
        assert_eq!(sink.next_sequence(), 4);

        let r1 = rx.try_recv().unwrap();
        assert_eq!(r1.sequence, 1);
        assert_eq!(r1.source, DialogSource::SessionAutoname);

        let r2 = rx.try_recv().unwrap();
        assert_eq!(r2.sequence, 2);
        assert_eq!(r2.source, DialogSource::SessionCompression);

        let r3 = rx.try_recv().unwrap();
        assert_eq!(r3.sequence, 3);
        assert_eq!(r3.source, DialogSource::ShellExecute);
    }
}


