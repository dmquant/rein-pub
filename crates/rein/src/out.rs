//! The CLI output contract (§9): stdout is machine output only; diagnostics
//! go to stderr; a stable JSON envelope; `ok` is defined as exactly
//! `exit code == 0`.

use rein_core::outcome::ExitCode;
use serde::Serialize;
use serde_json::{json, Value};

pub const ENVELOPE_SCHEMA: &str = "rein.cli-result/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Ndjson,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "yaml" => Ok(Self::Yaml),
            "ndjson" => Ok(Self::Ndjson),
            other => Err(format!("unknown output format `{other}`")),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub schema: &'static str,
    pub command: String,
    pub ok: bool,
    pub request_id: String,
    pub data: Value,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<String>,
    pub at: String,
}

pub struct CmdOutput {
    pub data: Value,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<String>,
    pub exit: ExitCode,
}

impl CmdOutput {
    pub fn ok(data: Value) -> Self {
        Self {
            data,
            warnings: Vec::new(),
            errors: Vec::new(),
            next_actions: Vec::new(),
            exit: ExitCode::AssertedTrue,
        }
    }

    pub fn with_exit(mut self, exit: ExitCode) -> Self {
        self.exit = exit;
        self
    }

    pub fn warn(mut self, w: impl Into<String>) -> Self {
        self.warnings.push(w.into());
        self
    }

    pub fn next(mut self, n: impl Into<String>) -> Self {
        self.next_actions.push(n.into());
        self
    }

    pub fn error(exit: ExitCode, message: impl Into<String>) -> Self {
        Self {
            data: Value::Null,
            warnings: Vec::new(),
            errors: vec![message.into()],
            next_actions: Vec::new(),
            exit,
        }
    }
}

/// Render the envelope to stdout in the chosen format; returns the process
/// exit code. `ok` is `exit == 0` by definition — nothing else.
pub fn emit(command: &str, out: CmdOutput, format: OutputFormat, at: String) -> i32 {
    let exit = out.exit.code();
    let envelope = Envelope {
        schema: ENVELOPE_SCHEMA,
        command: command.to_string(),
        ok: exit == 0,
        request_id: format!("req_{}", std::process::id()),
        data: out.data,
        warnings: out.warnings,
        errors: out.errors,
        next_actions: out.next_actions,
        at,
    };
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&envelope).expect("envelope serializes")
            );
        }
        OutputFormat::Yaml => {
            print!(
                "{}",
                serde_yaml::to_string(&envelope).expect("envelope serializes")
            );
        }
        OutputFormat::Ndjson => {
            match &envelope.data {
                Value::Array(items) => {
                    for item in items {
                        println!("{}", serde_json::to_string(item).expect("serializes"));
                    }
                }
                other => println!("{}", serde_json::to_string(other).expect("serializes")),
            }
            if !envelope.ok {
                for e in &envelope.errors {
                    eprintln!("error: {e}");
                }
            }
        }
        OutputFormat::Table => {
            render_table(&envelope.data);
            for w in &envelope.warnings {
                eprintln!("warning: {w}");
            }
            for e in &envelope.errors {
                eprintln!("error: {e}");
            }
            for n in &envelope.next_actions {
                eprintln!("next: {n}");
            }
        }
    }
    exit
}

/// Generic table rendering: array-of-flat-objects → columns; object → k/v
/// rows; scalar → itself. Deliberately simple — the TUI is the rich surface.
fn render_table(v: &Value) {
    match v {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {
            let mut cols: Vec<String> = Vec::new();
            for item in items {
                for k in item.as_object().expect("checked").keys() {
                    if !cols.contains(k) {
                        cols.push(k.clone());
                    }
                }
            }
            let cell = |item: &Value, c: &str| -> String {
                match item.get(c) {
                    None | Some(Value::Null) => "—".to_string(),
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                }
            };
            let mut widths: Vec<usize> = cols.iter().map(String::len).collect();
            for item in items {
                for (i, c) in cols.iter().enumerate() {
                    widths[i] = widths[i].max(cell(item, c).chars().count());
                }
            }
            let header: Vec<String> = cols
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{:w$}", c, w = widths[i]))
                .collect();
            println!("{}", header.join("  "));
            for item in items {
                let row: Vec<String> = cols
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{:w$}", cell(item, c), w = widths[i]))
                    .collect();
                println!("{}", row.join("  "));
            }
        }
        Value::Array(items) => {
            for item in items {
                println!("{item}");
            }
        }
        Value::Object(map) => {
            let w = map.keys().map(|k| k.len()).max().unwrap_or(0);
            for (k, val) in map {
                match val {
                    Value::String(s) => println!("{k:w$}  {s}"),
                    other => println!("{k:w$}  {other}"),
                }
            }
        }
        Value::Null => println!("(nothing)"),
        other => println!("{other}"),
    }
}

pub fn j<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

pub fn kv(pairs: &[(&str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        map.insert((*k).to_string(), v.clone());
    }
    Value::Object(map)
}

pub fn s(v: impl Into<String>) -> Value {
    json!(v.into())
}
