//! Streamable HTTP (text/event-stream) handling.
//!
//! MCP servers using the Streamable HTTP transport return responses
//! as Server-Sent Events: each JSON-RPC reply is wrapped in
//! `event: message\ndata: <json>\n\n`. The policy needs to (a)
//! recognise the SSE wrapper, (b) transform the JSON inside each
//! `data:` block when applicable, and (c) re-emit valid SSE so the
//! client's MCP transport keeps working.
//!
//! Per-event scope (no cross-event state). `event:`, `id:`, `retry:`,
//! and comment lines are preserved verbatim. Multi-line `data:` is
//! concatenated with `\n` per the SSE spec. Heartbeats and frames
//! whose `data:` is not JSON pass through untouched.

use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct Event {
    pub leading: Vec<String>,
    pub event: Option<String>,
    pub id: Option<String>,
    pub retry: Option<String>,
    pub data: String,
    pub comments: Vec<String>,
}

pub fn is_sse(content_type: Option<&str>) -> bool {
    content_type
        .map(|s| s.to_ascii_lowercase().starts_with("text/event-stream"))
        .unwrap_or(false)
}

pub fn parse(body: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut current = Event::default();
    let mut data_buf: Vec<String> = Vec::new();
    let mut has_field = false;

    for line in body.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);

        if line.is_empty() {
            if has_field || !data_buf.is_empty() {
                current.data = data_buf.join("\n");
                out.push(std::mem::take(&mut current));
                data_buf = Vec::new();
                has_field = false;
            } else {
                current.leading.push(String::new());
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix(':') {
            current.comments.push(rest.trim_start().to_string());
            has_field = true;
            continue;
        }

        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };

        match field {
            "event" => current.event = Some(value.to_string()),
            "id" => current.id = Some(value.to_string()),
            "retry" => current.retry = Some(value.to_string()),
            "data" => data_buf.push(value.to_string()),
            _ => { /* unknown field — per spec, ignore */ }
        }
        has_field = true;
    }

    if has_field || !data_buf.is_empty() {
        current.data = data_buf.join("\n");
        out.push(current);
    }

    out
}

pub fn data_as_json(event: &Event) -> Option<Value> {
    if event.data.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&event.data).ok()
}

pub fn render_event(event: &Event, data_json: &Value) -> String {
    let mut out = String::new();
    write_prelude(event, &mut out);
    let serialized = serde_json::to_string(data_json).unwrap_or_else(|_| event.data.clone());
    for chunk in serialized.split('\n') {
        out.push_str("data: ");
        out.push_str(chunk);
        out.push('\n');
    }
    out.push('\n');
    out
}

pub fn render_event_passthrough(event: &Event) -> String {
    let mut out = String::new();
    write_prelude(event, &mut out);
    if !event.data.is_empty() {
        for chunk in event.data.split('\n') {
            out.push_str("data: ");
            out.push_str(chunk);
            out.push('\n');
        }
    }
    out.push('\n');
    out
}

fn write_prelude(event: &Event, out: &mut String) {
    for ln in &event.leading {
        out.push_str(ln);
        out.push('\n');
    }
    if let Some(ref ev) = event.event {
        out.push_str("event: ");
        out.push_str(ev);
        out.push('\n');
    }
    if let Some(ref id) = event.id {
        out.push_str("id: ");
        out.push_str(id);
        out.push('\n');
    }
    if let Some(ref retry) = event.retry {
        out.push_str("retry: ");
        out.push_str(retry);
        out.push('\n');
    }
    for c in &event.comments {
        out.push(':');
        if !c.is_empty() {
            out.push(' ');
            out.push_str(c);
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_event() {
        let body = "event: message\ndata: {\"a\":1}\n\n";
        let events = parse(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("message"));
        assert_eq!(events[0].data, "{\"a\":1}");
    }

    #[test]
    fn parses_multi_data_event() {
        let body = "event: message\ndata: line1\ndata: line2\n\n";
        let events = parse(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }

    #[test]
    fn handles_crlf() {
        let body = "event: message\r\ndata: {\"a\":1}\r\n\r\n";
        let events = parse(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"a\":1}");
    }

    #[test]
    fn round_trip_passthrough() {
        let body = "event: message\nid: 1\ndata: {\"x\":1}\n\n";
        let events = parse(body);
        let out = render_event_passthrough(&events[0]);
        assert!(out.contains("event: message"));
        assert!(out.contains("id: 1"));
        assert!(out.contains("data: {\"x\":1}"));
        assert!(out.ends_with("\n\n"));
    }

    #[test]
    fn render_with_replacement_data() {
        let event = parse("event: message\ndata: {\"x\":1}\n\n")
            .into_iter()
            .next()
            .unwrap();
        let out = render_event(&event, &serde_json::json!({"y":2}));
        assert!(out.contains("event: message"));
        assert!(out.contains("data: {\"y\":2}"));
    }

    #[test]
    fn data_as_json_returns_none_for_heartbeat() {
        let events = parse(": ping\n\n");
        assert!(data_as_json(&events[0]).is_none());
    }
}
