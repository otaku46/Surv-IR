use super::types::{FoundSymbol, SymbolRange};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{BufRead, BufReader, Write};
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
                    return Err(
                        "rust-analyzer not found. Install with: rustup component add rust-analyzer"
                            .into(),
                    );
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
        let canonical_root = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        let workspace_uri = format!("file://{}", canonical_root.display());

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

        let req_id = self.send_request("initialize", init_params)?;
        let _response = self.read_response(req_id)?;

        // Send initialized notification
        self.send_notification("initialized", serde_json::json!({}))?;

        Ok(())
    }

    /// Query workspace symbols
    pub fn workspace_symbol(&mut self, query: &str) -> Result<Vec<FoundSymbol>, Box<dyn Error>> {
        let params = serde_json::json!({
            "query": query
        });

        let req_id = self.send_request("workspace/symbol", params)?;
        let response = self.read_response(req_id)?;

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

        let container_name = value
            .get("containerName")
            .and_then(|v| v.as_str())
            .map(String::from);

        Some(FoundSymbol {
            language: None,
            name,
            kind: kind_str,
            uri,
            relative_path: None,
            module_path: None,
            impl_path: None,
            range: symbol_range,
            container_name,
            visibility: None,
            is_test: false,
            is_method: false,
            detail: None,
            alias_target: None,
            enum_variants: Vec::new(),
            enum_variants_known: false,
            signature: None,
            fields: BTreeMap::new(),
            fields_known: false,
        })
    }

    fn send_request(&mut self, method: &str, params: Value) -> Result<i64, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.write_message(&request)?;
        Ok(id)
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

    fn read_response(&mut self, expected_id: i64) -> Result<Value, Box<dyn Error>> {
        read_response_from_reader(&mut self.stdout, expected_id)
    }

    pub fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        // Do not wait for a shutdown response: a stuck or half-initialized
        // server must not be able to block the CLI indefinitely.
        let _ = self.send_request("shutdown", serde_json::json!(null));
        let _ = self.send_notification("exit", serde_json::json!({}));
        let _ = self.process.kill();
        let _ = self.process.wait();
        Ok(())
    }
}

fn read_message_from_reader<R: BufRead>(reader: &mut R) -> Result<Value, Box<dyn Error>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Err("LSP server closed connection unexpectedly (EOF)".into());
        }

        if line == "\r\n" || line == "\n" {
            if content_length.is_some() {
                break;
            }
            continue;
        }

        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = Some(value.trim().parse()?);
            }
        }
    }

    let length = content_length.ok_or("Missing Content-Length header in LSP message")?;
    let mut buffer = vec![0u8; length];
    reader.read_exact(&mut buffer)?;

    let message: Value = serde_json::from_slice(&buffer)?;
    Ok(message)
}

fn read_response_from_reader<R: BufRead>(
    reader: &mut R,
    expected_id: i64,
) -> Result<Value, Box<dyn Error>> {
    loop {
        let message = read_message_from_reader(reader)?;
        if message.get("id").and_then(|v| v.as_i64()) == Some(expected_id) {
            // Skip server-initiated requests that happen to carry a matching id AND a method field
            if message.get("method").is_some() {
                continue;
            }
            if let Some(err) = message.get("error") {
                return Err(format!("LSP request {} failed: {}", expected_id, err).into());
            }
            return Ok(message);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame_lsp_message(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    #[test]
    fn skips_notifications_and_returns_matching_response() {
        let notif = frame_lsp_message(
            r#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"indexing"}}"#,
        );
        let other_resp = frame_lsp_message(r#"{"jsonrpc":"2.0","id":1,"result":null}"#);
        let target_resp =
            frame_lsp_message(r#"{"jsonrpc":"2.0","id":2,"result":[{"name":"foo"}]}"#);
        let stream = format!("{notif}{other_resp}{target_resp}");

        let mut reader = Cursor::new(stream.into_bytes());
        let response = read_response_from_reader(&mut reader, 2).expect("matching response");
        assert_eq!(response["id"], 2);
        assert_eq!(response["result"][0]["name"], "foo");
    }

    #[test]
    fn returns_error_on_eof_instead_of_looping() {
        let mut reader = Cursor::new(Vec::<u8>::new());
        let err = read_response_from_reader(&mut reader, 1).expect_err("should fail on EOF");
        assert!(err.to_string().contains("EOF"));
    }
}
