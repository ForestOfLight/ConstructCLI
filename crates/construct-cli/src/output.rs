//! The two output channels.
//!
//! Under `--json`, stdout carries exactly one JSON document and nothing else,
//! so a consumer can parse it without scanning. Warnings and progress go to
//! stderr as plain text and are *also* embedded in the payload: a GUI linking
//! construct-core gets them as values, a terminal user gets them as they
//! happen, and neither has to read the other stream.

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
        let mut map = match serde_json::to_value(payload) {
            Ok(Value::Object(m)) => m,
            other => {
                let mut m = Map::new();
                m.insert("value".into(), other.unwrap_or(Value::Null));
                m
            }
        };
        map.insert("schema".into(), json!(1));
        map.insert("warnings".into(), json!(self.warnings));
        println!("{}", Value::Object(map));
    }
}
