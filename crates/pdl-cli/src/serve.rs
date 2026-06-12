use pdl_core::{has_errors, Diagnostic};
use pdl_data::Value;
use pdl_driver::{prepare_file, OsDriverIo};
use pdl_exec::{run_prepared_with_io_and_context_and_engine, ExecutionEngine, RunOptions};
use serde_json::{json, Number, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

pub fn serve(file: PathBuf, host: String, port: u16) -> Result<ExitCode, String> {
    let listener = TcpListener::bind((host.as_str(), port))
        .map_err(|error| format!("could not bind {host}:{port}: {error}"))?;
    let addr = listener
        .local_addr()
        .map_err(|error| format!("could not inspect local address: {error}"))?;
    let state = Arc::new(Mutex::new(ServerState {
        file,
        context: BTreeMap::new(),
        revision: 0,
        snapshot: None,
        source_modified: None,
    }));
    println!("pdl serve listening at http://{addr}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = handle_connection(stream, state);
                });
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Clone)]
struct ServerState {
    file: PathBuf,
    context: BTreeMap<String, Value>,
    revision: u64,
    snapshot: Option<JsonValue>,
    source_modified: Option<SystemTime>,
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) -> Result<(), String> {
    let request = read_request(&stream)?;
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => respond(&mut stream, 200, "text/html; charset=utf-8", APP_HTML),
        ("GET", "/favicon.ico") => respond(&mut stream, 204, "image/x-icon", ""),
        ("GET", "/api/snapshot") => {
            let snapshot = cached_snapshot_or_refresh(&state);
            respond_json(&mut stream, 200, &snapshot)
        }
        ("GET", "/events") => stream_events(stream, state),
        ("POST", "/api/context") => {
            let updates = parse_context_body(&request.body)?;
            let snapshot = {
                let mut state = state
                    .lock()
                    .map_err(|_| "server state lock poisoned".to_string())?;
                let mut changed = false;
                for (name, value) in updates {
                    if state.context.get(&name) != Some(&value) {
                        state.context.insert(name, value);
                        changed = true;
                    }
                }
                if changed {
                    drop_stale_context(&mut state);
                    refresh_snapshot_locked(&mut state)
                } else {
                    cached_snapshot_locked(&mut state)
                }
            };
            respond_json(&mut stream, 200, &snapshot)
        }
        ("POST", "/api/run") => {
            let snapshot = refresh_snapshot(&state);
            respond_json(&mut stream, 200, &snapshot)
        }
        _ => respond(&mut stream, 404, "text/plain; charset=utf-8", "not found\n"),
    }
}

fn stream_events(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) -> Result<(), String> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n"
    )
    .map_err(|error| format!("response write failed: {error}"))?;
    let mut last_revision = None;
    loop {
        let (revision, snapshot) = {
            let mut state = state
                .lock()
                .map_err(|_| "server state lock poisoned".to_string())?;
            let snapshot = cached_snapshot_locked(&mut state);
            (state.revision, snapshot)
        };
        if last_revision != Some(revision) {
            let payload = serde_json::to_string(&snapshot)
                .map_err(|error| format!("snapshot serialization failed: {error}"))?;
            if write!(stream, "data: {payload}\n\n").is_err() {
                break;
            }
            last_revision = Some(revision);
        } else if write!(stream, ": keep-alive\n\n").is_err() {
            break;
        }
        let _ = stream.flush();
        thread::sleep(Duration::from_millis(750));
    }
    Ok(())
}

fn cached_snapshot_or_refresh(state: &Arc<Mutex<ServerState>>) -> JsonValue {
    match state.lock() {
        Ok(mut state) => cached_snapshot_locked(&mut state),
        Err(_) => lock_error_snapshot(),
    }
}

fn refresh_snapshot(state: &Arc<Mutex<ServerState>>) -> JsonValue {
    match state.lock() {
        Ok(mut state) => refresh_snapshot_locked(&mut state),
        Err(_) => lock_error_snapshot(),
    }
}

fn cached_snapshot_locked(state: &mut ServerState) -> JsonValue {
    if state.snapshot.is_none() || source_changed(state) {
        refresh_snapshot_locked(state)
    } else {
        state
            .snapshot
            .clone()
            .unwrap_or_else(|| refresh_snapshot_locked(state))
    }
}

fn refresh_snapshot_locked(state: &mut ServerState) -> JsonValue {
    state.revision = state.revision.saturating_add(1);
    state.source_modified = file_modified(&state.file);
    let mut snapshot = build_snapshot_json(&state.file, &state.context);
    set_revision(&mut snapshot, state.revision);
    state.snapshot = Some(snapshot.clone());
    snapshot
}

fn build_snapshot_json(file: &PathBuf, context: &BTreeMap<String, Value>) -> JsonValue {
    let prepared = match prepare_file(file) {
        Ok(prepared) => prepared,
        Err(diagnostic) => {
            return json!({
                "source_path": file.display().to_string(),
                "status": "failed",
                "context": context_json(context),
                "controls": null,
                "diagnostics": [diagnostic],
                "run": null
            });
        }
    };
    let controls = crate::render::controls_json(&prepared, context.clone());
    let mut diagnostics = controls.diagnostics().to_vec();
    let run = if has_errors(&diagnostics) {
        None
    } else {
        let io = OsDriverIo;
        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            context.clone(),
            ExecutionEngine::Auto,
        );
        extend_new_diagnostics(&mut diagnostics, result.diagnostics);
        Some(json!({
            "backend": format!("{:?}", result.backend),
            "named_outputs": result.named_outputs.iter().map(|output| output.name.clone()).collect::<Vec<_>>(),
            "stdout_bytes": result.stdout.as_ref().map_or(0, Vec::len)
        }))
    };
    let status = if has_errors(&diagnostics) {
        "failed"
    } else {
        "succeeded"
    };
    json!({
        "source_path": prepared.path.display().to_string(),
        "status": status,
        "context": context_json(context),
        "controls": controls,
        "diagnostics": diagnostics,
        "run": run
    })
}

fn set_revision(snapshot: &mut JsonValue, revision: u64) {
    if let JsonValue::Object(object) = snapshot {
        object.insert(
            "revision".to_string(),
            JsonValue::Number(Number::from(revision)),
        );
    }
}

fn file_modified(path: &PathBuf) -> Option<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn source_changed(state: &ServerState) -> bool {
    state.source_modified != file_modified(&state.file)
}

fn lock_error_snapshot() -> JsonValue {
    json!({
        "status": "failed",
        "diagnostics": [{
            "severity": "error",
            "code": "E1505",
            "message": "server state lock poisoned",
            "span": { "start": 0, "end": 0 }
        }]
    })
}

fn drop_stale_context(state: &mut ServerState) {
    if let Ok(prepared) = prepare_file(&state.file) {
        if let Some(ir) = prepared.analysis.ir {
            let names = ir
                .contexts
                .into_iter()
                .map(|context| context.name)
                .collect::<Vec<_>>();
            state
                .context
                .retain(|name, _| names.iter().any(|known| known == name));
        }
    }
}

fn read_request(stream: &TcpStream) -> Result<HttpRequest, String> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("request read failed: {error}"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| format!("header read failed: {error}"))?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|error| format!("body read failed: {error}"))?;
    }
    Ok(HttpRequest { method, path, body })
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn parse_context_body(body: &[u8]) -> Result<BTreeMap<String, Value>, String> {
    let json: JsonValue =
        serde_json::from_slice(body).map_err(|error| format!("invalid JSON body: {error}"))?;
    let object = json
        .get("context")
        .and_then(JsonValue::as_object)
        .or_else(|| json.as_object())
        .ok_or_else(|| "context body must be a JSON object".to_string())?;
    object
        .iter()
        .map(|(name, value)| Ok((name.clone(), json_context_value(value)?)))
        .collect()
}

fn json_context_value(value: &JsonValue) -> Result<Value, String> {
    match value {
        JsonValue::Null => Ok(Value::Null),
        JsonValue::Bool(value) => Ok(Value::Bool(*value)),
        JsonValue::Number(value) => value
            .as_f64()
            .map(Value::Number)
            .ok_or_else(|| "context number is outside f64 range".to_string()),
        JsonValue::String(value) => Ok(Value::String(value.clone())),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            Err("context values must be null, boolean, number, or string".to_string())
        }
    }
}

fn context_json(context: &BTreeMap<String, Value>) -> JsonValue {
    JsonValue::Object(
        context
            .iter()
            .map(|(name, value)| (name.clone(), value_json(value)))
            .collect(),
    )
}

fn value_json(value: &Value) -> JsonValue {
    match value {
        Value::Null => JsonValue::Null,
        Value::Bool(value) => JsonValue::Bool(*value),
        Value::Number(value) => Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::String(value) => JsonValue::String(value.clone()),
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        status_text(status),
        body.len()
    )
    .map_err(|error| format!("response write failed: {error}"))
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        _ => "OK",
    }
}

fn respond_json(stream: &mut TcpStream, status: u16, body: &JsonValue) -> Result<(), String> {
    let body = serde_json::to_string_pretty(body)
        .map_err(|error| format!("json serialization failed: {error}"))?;
    respond(stream, status, "application/json; charset=utf-8", &body)
}

fn extend_new_diagnostics(target: &mut Vec<Diagnostic>, incoming: Vec<Diagnostic>) {
    for diagnostic in incoming {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

const APP_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>PDL Serve</title>
  <style>
    :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    body { margin: 0; background: Canvas; color: CanvasText; }
    main { display: grid; grid-template-columns: minmax(260px, 380px) 1fr; min-height: 100vh; }
    aside { border-right: 1px solid color-mix(in srgb, CanvasText 18%, transparent); padding: 18px; overflow: auto; }
    section { padding: 18px; overflow: auto; }
    h1 { font-size: 18px; margin: 0 0 14px; }
    label { display: block; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
    .control { margin: 0 0 16px; }
    input, select, textarea, button { box-sizing: border-box; width: 100%; font: inherit; padding: 8px; border: 1px solid color-mix(in srgb, CanvasText 26%, transparent); border-radius: 6px; background: Canvas; color: CanvasText; }
    input[type="checkbox"], input[type="radio"], input[type="color"] { width: auto; }
    .row { display: flex; align-items: center; gap: 8px; }
    .radio-row { display: flex; align-items: center; gap: 8px; margin: 5px 0; font-weight: 400; }
    .status { font-size: 13px; margin-bottom: 16px; }
    pre { white-space: pre-wrap; padding: 12px; background: color-mix(in srgb, CanvasText 8%, transparent); border-radius: 6px; }
    @media (max-width: 760px) { main { grid-template-columns: 1fr; } aside { border-right: 0; border-bottom: 1px solid color-mix(in srgb, CanvasText 18%, transparent); } }
  </style>
</head>
<body>
  <main>
    <aside>
      <h1>PDL Controls</h1>
      <div id="status" class="status"></div>
      <div id="controls"></div>
      <button id="run">Run</button>
    </aside>
    <section>
      <h1>Diagnostics</h1>
      <pre id="diagnostics"></pre>
    </section>
  </main>
  <script>
    let latest = null;
    let renderedRevision = null;
    let renderedControlsSignature = '';
    const controlNodes = new Map();
    const controlsEl = document.getElementById('controls');
    const statusEl = document.getElementById('status');
    const diagnosticsEl = document.getElementById('diagnostics');
    document.getElementById('run').onclick = () => fetch('/api/run', { method: 'POST' }).then(r => r.json()).then(applySnapshot);
    new EventSource('/events').onmessage = event => applySnapshot(JSON.parse(event.data));
    fetch('/api/snapshot').then(r => r.json()).then(applySnapshot);
    function applySnapshot(snapshot) {
      if (snapshot.revision !== undefined && snapshot.revision === renderedRevision) return;
      latest = snapshot;
      renderedRevision = snapshot.revision ?? renderedRevision;
      statusEl.textContent = snapshot.status || 'pending';
      const diagnostics = snapshot.diagnostics || [];
      diagnosticsEl.textContent = diagnostics.length ? diagnostics.map(d => `${d.severity || 'error'}[${d.code}]: ${d.message}`).join('\n') : 'No diagnostics';
      const list = snapshot.controls && snapshot.controls.controls || [];
      const signature = controlsSignature(list);
      if (signature !== renderedControlsSignature) {
        renderedControlsSignature = signature;
        controlNodes.clear();
        controlsEl.innerHTML = '';
        for (const control of list) controlsEl.appendChild(renderControl(control));
      } else {
        for (const control of list) syncControlValue(control);
      }
    }
    function renderControl(control) {
      const wrap = document.createElement('div');
      wrap.className = 'control';
      wrap.dataset.controlName = control.name;
      controlNodes.set(control.name, wrap);
      const label = document.createElement('label');
      label.textContent = control.label || control.name;
      wrap.appendChild(label);
      const value = controlValue(control);
      if (control.kind === 'input_checkbox') {
        const row = document.createElement('div');
        row.className = 'row';
        const input = document.createElement('input');
        input.type = 'checkbox';
        input.checked = value === true;
        input.onchange = () => update(control.name, input.checked);
        row.appendChild(input);
        wrap.appendChild(row);
        return wrap;
      }
      if (control.kind === 'input_radio') {
        for (const choice of mergedChoices(control)) {
          const row = document.createElement('label');
          row.className = 'radio-row';
          const input = document.createElement('input');
          input.type = 'radio';
          input.name = control.name;
          input.value = JSON.stringify(choice);
          input.checked = JSON.stringify(choice) === JSON.stringify(value);
          input.onchange = () => update(control.name, choice);
          row.appendChild(input);
          row.appendChild(document.createTextNode(String(choice)));
          wrap.appendChild(row);
        }
        return wrap;
      }
      const input = document.createElement(control.kind === 'input_textarea' ? 'textarea' : control.kind === 'input_select' ? 'select' : 'input');
      if (control.kind === 'input_text') input.type = 'text';
      if (control.kind === 'input_number') input.type = 'number';
      if (control.kind === 'input_range') input.type = 'range';
      if (control.kind === 'input_date') input.type = 'date';
      if (control.kind === 'input_time') input.type = 'time';
      if (control.kind === 'input_datetime') input.type = 'datetime-local';
      if (control.kind === 'input_color') input.type = 'color';
      if (control.placeholder) input.placeholder = control.placeholder;
      if (control.rows) input.rows = control.rows;
      for (const attr of ['min', 'max', 'step']) if (control[attr] !== undefined) input[attr] = control[attr];
      if (control.kind === 'input_select') {
        for (const choice of mergedChoices(control)) {
          const option = document.createElement('option');
          option.value = JSON.stringify(choice);
          option.textContent = String(choice);
          option.selected = JSON.stringify(choice) === JSON.stringify(value);
          input.appendChild(option);
        }
        input.onchange = () => update(control.name, JSON.parse(input.value));
      } else {
        input.value = value ?? '';
        input.oninput = () => update(control.name, input.type === 'number' || input.type === 'range' ? Number(input.value) : input.value);
      }
      wrap.appendChild(input);
      return wrap;
    }
    function syncControlValue(control) {
      const node = controlNodes.get(control.name);
      if (!node || node.contains(document.activeElement)) return;
      const value = controlValue(control);
      if (control.kind === 'input_checkbox') {
        const input = node.querySelector('input');
        if (input) input.checked = value === true;
        return;
      }
      if (control.kind === 'input_radio') {
        for (const input of node.querySelectorAll('input[type="radio"]')) input.checked = input.value === JSON.stringify(value);
        return;
      }
      const input = node.querySelector('input, select, textarea');
      if (!input) return;
      const next = control.kind === 'input_select' ? JSON.stringify(value) : value ?? '';
      if (input.value !== String(next)) input.value = next;
    }
    function controlValue(control) {
      return control.current_value !== undefined ? control.current_value : control.default;
    }
    function controlsSignature(list) {
      return JSON.stringify(list.map(control => ({
        name: control.name,
        kind: control.kind,
        label: control.label,
        placeholder: control.placeholder,
        rows: control.rows,
        min: control.min,
        max: control.max,
        step: control.step,
        choices: control.kind === 'input_select' || control.kind === 'input_radio' ? mergedChoices(control) : null
      })));
    }
    function mergedChoices(control) {
      const values = [];
      const seen = new Set();
      for (const item of control.choices || []) push(item.value);
      for (const item of (control.dynamic_choices && control.dynamic_choices.choices) || []) push(item);
      const value = controlValue(control);
      if (value !== undefined && !seen.has(JSON.stringify(value))) values.unshift(value);
      return values;
      function push(value) {
        const key = JSON.stringify(value);
        if (!seen.has(key)) { seen.add(key); values.push(value); }
      }
    }
    let timer = null;
    function update(name, value) {
      clearTimeout(timer);
      timer = setTimeout(() => fetch('/api/context', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ [name]: value })
      }).then(r => r.json()).then(applySnapshot), 150);
    }
  </script>
</body>
</html>
"#;
