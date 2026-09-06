//! The two output channels.
//!
//! Under `--json`, stdout carries exactly one JSON document and nothing else,
//! so a consumer can parse it without scanning. That holds for failures too:
//! a failed command emits a document carrying `error`, because the exit code
//! is only 0/1/2 and has no room to say *what* went wrong. Warnings and
//! progress go to stderr as plain text and are *also* embedded in the payload:
//! a GUI linking construct-core gets them as values, a terminal user gets them
//! as they happen, and neither has to read the other stream.

use construct_core::error::CoreError;
use serde::Serialize;
use serde_json::{Map, Value, json};

pub struct Out {
    json: bool,
    warnings: Vec<String>,
}

impl Out {
    pub fn new(json: bool) -> Self {
        Self {
            json,
            warnings: Vec::new(),
        }
    }

    pub fn is_json(&self) -> bool {
        self.json
    }

    /// Records a warning and shows it immediately on stderr.
    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        eprintln!("warning: {msg}");
        self.warnings.push(msg);
    }

    /// Emits a human line, suppressed under `--json`.
    pub fn line(&self, text: impl AsRef<str>) {
        if !self.json {
            println!("{}", text.as_ref());
        }
    }

    /// Emits the single JSON document. Does nothing when not in JSON mode.
    pub fn emit<T: Serialize>(&self, payload: T) {
        if !self.json {
            return;
        }
        self.write(to_map(payload), None);
    }

    /// Emits the single JSON document for a failure: no payload, just `error`.
    ///
    /// Callers branch on `error.kind` — the exit code says only whether it
    /// worked and whether the input was at fault, so this is where the reason
    /// lives.
    pub fn emit_error(&self, err: &CoreError) {
        if !self.json {
            return;
        }
        self.write(Map::new(), Some((err.kind(), err.to_string())));
    }

    /// Emits a payload *and* an `error`, in one document.
    ///
    /// For the failure that still has something worth reporting: `install`
    /// placing the packs and then failing a later step has real paths and a
    /// version the caller needs in order to recover, and stdout is exactly one
    /// document, so the two cannot be emitted separately. Merging keeps
    /// "nonzero exit implies `error.kind`" true with no command-specific
    /// exception for a caller to learn.
    pub fn emit_with_error<T: Serialize>(&self, payload: T, kind: &str, message: &str) {
        if !self.json {
            return;
        }
        self.write(to_map(payload), Some((kind, message.to_string())));
    }

    /// The one place `schema` and `warnings` are injected, and the one place
    /// the document is printed.
    fn write(&self, mut map: Map<String, Value>, error: Option<(&str, String)>) {
        if let Some((kind, message)) = error {
            map.insert("error".into(), json!({ "kind": kind, "message": message }));
        }
        map.insert("schema".into(), json!(1));
        map.insert("warnings".into(), json!(self.warnings));
        println!("{}", Value::Object(map));
    }
}

/// A payload as an object, so `schema`, `warnings`, and `error` have somewhere
/// to be inserted. A payload that is not an object is nested under `value`
/// rather than dropped.
fn to_map<T: Serialize>(payload: T) -> Map<String, Value> {
    match serde_json::to_value(payload) {
        Ok(Value::Object(m)) => m,
        other => {
            let mut m = Map::new();
            m.insert("value".into(), other.unwrap_or(Value::Null));
            m
        }
    }
}
