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

    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        eprintln!("warning: {msg}");
        self.warnings.push(msg);
    }

    pub fn line(&self, text: impl AsRef<str>) {
        if !self.json {
            println!("{}", text.as_ref());
        }
    }

    pub fn emit<T: Serialize>(&self, payload: T) {
        if !self.json {
            return;
        }
        self.write(to_map(payload), None);
    }

    pub fn emit_error(&self, err: &CoreError) {
        if !self.json {
            return;
        }
        self.write(Map::new(), Some((err.kind(), err.to_string())));
    }

    pub fn emit_with_error<T: Serialize>(&self, payload: T, kind: &str, message: &str) {
        if !self.json {
            return;
        }
        self.write(to_map(payload), Some((kind, message.to_string())));
    }

    fn write(&self, mut map: Map<String, Value>, error: Option<(&str, String)>) {
        if let Some((kind, message)) = error {
            map.insert("error".into(), json!({ "kind": kind, "message": message }));
        }
        map.insert("schema".into(), json!(1));
        map.insert("warnings".into(), json!(self.warnings));
        println!("{}", Value::Object(map));
    }
}

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
