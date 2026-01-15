use super::types::{FoundSymbol, SymbolRange};
use serde_json::Value;
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct LspClient {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl LspClient {
    /// Create a new LSP client for the given language
    pub fn new(language: &str, workspace_root: &Path) -> Result<Self, Box<dyn Error>> {
        let (command, args) = match language {
            "ts" | "typescript" => {
                // Try typescript-language-server first, fallback to tsserver
                if which::which("typescript-language-server").is_ok() {
                    ("typescript-language-server", vec!["--stdio"])
                } else {
                    return Err("typescript-language-server not found. Install with: npm install -g typescript-language-server typescript".into());
                }
            }
            "rust" => {
                // Use rust-analyzer
                if which::which("rust-analyzer").is_ok() {
                    ("rust-analyzer", vec![])
                } else {
                    return Err("rust-analyzer not found. Install with: rustup component add rust-analyzer".into());
                }
            }
            _ => return Err(format!("Unsupported language: {}", language).into()),
        };

        let mut process = Command::new(command)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = process.stdin.take().ok_or("Failed to open stdin")?;
        let stdout = BufReader::new(process.stdout.take().ok_or("Failed to open stdout")?);

        let mut client = LspClient {
            process,
            stdin,
            stdout,
            next_id: 1,
        };

        // Initialize LSP connection
        client.initialize(workspace_root)?;

        Ok(client)
    }

    fn initialize(&mut self, workspace_root: &Path) -> Result<(), Box<dyn Error>> {
        let workspace_uri = format!("file://{}", workspace_root.display());

        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": workspace_uri,
            "capabilities": {
                "workspace": {
                    "workspaceSymbol": {
                        "dynamicRegistration": false
                    }
                }
            }
        });

        self.send_request("initialize", init_params)?;
        let _response = self.read_response()?;

        // Send initialized notification
        self.send_notification("initialized", serde_json::json!({}))?;

        Ok(())
    }

    /// Query workspace symbols
    pub fn workspace_symbol(&mut self, query: &str) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
        let params = serde_json::json!({
            "query": query
        });

        self.send_request("workspace/symbol", params)?;
        let response = self.read_response()?;

        // Parse response
        if let Some(result) = response.get("result") {
            if let Some(symbols) = result.as_array() {
                return Ok(symbols
                    .iter()
                    .filter_map(|sym| self.parse_symbol_information(sym))
                    .collect());
            }
        }

        Ok(Vec::new())
    }

    fn parse_symbol_information(&self, value: &Value) -> Option<FoundSymbol> {
        let name = value.get("name")?.as_str()?.to_string();
        let kind = value.get("kind")?.as_u64()?;
        let kind_str = symbol_kind_to_string(kind as i32);

        let location = value.get("location")?;
        let uri = location.get("uri")?.as_str()?.to_string();
        let range = location.get("range")?;

        let start = range.get("start")?;
        let end = range.get("end")?;

        let symbol_range = SymbolRange {
            start_line: start.get("line")?.as_u64()? as u32,
            start_char: start.get("character")?.as_u64()? as u32,
            end_line: end.get("line")?.as_u64()? as u32,
            end_char: end.get("character")?.as_u64()? as u32,
        };

        let container_name = value.get("containerName").and_then(|v| v.as_str()).map(String::from);

        Some(FoundSymbol {
            name,
            kind: kind_str,
            uri,
            range: symbol_range,
            container_name,
            detail: None,
        })
    }

    fn send_request(&mut self, method: &str, params: Value) -> Result<(), Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.write_message(&request)?;
        Ok(())
    }

    fn send_notification(&mut self, method: &str, params: Value) -> Result<(), Box<dyn Error>> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        self.write_message(&notification)?;
        Ok(())
    }

    fn write_message(&mut self, message: &Value) -> Result<(), Box<dyn Error>> {
        let content = serde_json::to_string(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", content.len());

        self.stdin.write_all(header.as_bytes())?;
        self.stdin.write_all(content.as_bytes())?;
        self.stdin.flush()?;

        Ok(())
    }

    fn read_response(&mut self) -> Result<Value, Box<dyn Error>> {
        // Read headers
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line)?;

            if line == "\r\n" {
                break;
            }

            if line.starts_with("Content-Length:") {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    content_length = parts[1].trim().parse()?;
                }
            }
        }

        // Read content
        let mut buffer = vec![0u8; content_length];
        self.stdout.read_exact(&mut buffer)?;

        let response: Value = serde_json::from_slice(&buffer)?;
        Ok(response)
    }

    pub fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        self.send_request("shutdown", serde_json::json!(null))?;
        let _response = self.read_response()?;

        self.send_notification("exit", serde_json::json!({}))?;

        let _ = self.process.wait();

        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn symbol_kind_to_string(kind: i32) -> String {
    match kind {
        1 => "File",
        2 => "Module",
        3 => "Namespace",
        4 => "Package",
        5 => "Class",
        6 => "Method",
        7 => "Property",
        8 => "Field",
        9 => "Constructor",
        10 => "Enum",
        11 => "Interface",
        12 => "Function",
        13 => "Variable",
        14 => "Constant",
        15 => "String",
        16 => "Number",
        17 => "Boolean",
        18 => "Array",
        19 => "Object",
        20 => "Key",
        21 => "Null",
        22 => "EnumMember",
        23 => "Struct",
        24 => "Event",
        25 => "Operator",
        26 => "TypeParameter",
        _ => "Unknown",
    }
    .to_string()
}
