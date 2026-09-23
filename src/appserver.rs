//! Minimal client for `codex app-server` (line-delimited JSON-RPC over stdio).
//!
//! This is the same interface the Codex app uses internally, so threads created
//! here are indistinguishable from ones created in the GUI.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub struct AppServer {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    next_id: u64,
    /// Messages received while waiting for something else.
    backlog: VecDeque<Value>,
}

#[derive(Debug, Default)]
pub struct TurnOutcome {
    /// `completed`, `interrupted` or `failed`.
    pub status: String,
    pub error: Option<String>,
    /// Text of the last agent message in the turn.
    pub reply: String,
    /// `error` notifications seen during the turn (including retried ones).
    pub errors: Vec<String>,
}

impl AppServer {
    pub fn spawn(codex: &Path, home: &Path, cwd: Option<&Path>) -> Result<Self> {
        let mut cmd = Command::new(codex);
        cmd.arg("app-server")
            .env("CODEX_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(dir) = cwd.filter(|d| d.is_dir()) {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("{} app-server を起動できません", codex.display()))?;
        let stdin = child
            .stdin
            .take()
            .context("app-server の stdin を取得できません")?;
        let stdout = child
            .stdout
            .take()
            .context("app-server の stdout を取得できません")?;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });

        let mut server = Self {
            child,
            stdin: Some(stdin),
            rx,
            next_id: 1,
            backlog: VecDeque::new(),
        };
        server.request(
            "initialize",
            json!({"clientInfo": {"name": "codex-switch", "version": env!("CARGO_PKG_VERSION")}}),
            Duration::from_secs(60),
        )?;
        Ok(server)
    }

    pub fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + timeout;
        loop {
            let Some(msg) = self.recv(deadline)? else {
                bail!("{method} の応答がタイムアウトしました");
            };
            if self.answer_server_request(&msg)? {
                continue;
            }
            if msg.get("method").is_none() && msg.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = msg.get("error") {
                    let text = err["message"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| err.to_string());
                    bail!("{method} が失敗しました: {text}");
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }
            self.backlog.push_back(msg);
        }
    }

    /// Waits for `turn/completed` on `thread_id`, collecting the agent's reply.
    pub fn wait_turn(&mut self, thread_id: &str, timeout: Duration) -> Result<TurnOutcome> {
        let deadline = Instant::now() + timeout;
        let mut out = TurnOutcome::default();
        loop {
            let msg = match self.backlog.pop_front() {
                Some(m) => m,
                None => match self.recv(deadline)? {
                    Some(m) => m,
                    None => bail!(
                        "ターンの完了待ちがタイムアウトしました（{} 秒）",
                        timeout.as_secs()
                    ),
                },
            };
            if self.answer_server_request(&msg)? {
                continue;
            }
            let params = &msg["params"];
            if params["threadId"].as_str() != Some(thread_id) {
                continue;
            }
            match msg["method"].as_str().unwrap_or("") {
                "item/completed" if params["item"]["type"] == "agentMessage" => {
                    if let Some(text) = params["item"]["text"].as_str() {
                        out.reply = text.to_owned();
                    }
                }
                "error" => {
                    if let Some(text) = params["error"]["message"].as_str() {
                        out.errors.push(text.to_owned());
                    }
                }
                "turn/completed" => {
                    let turn = &params["turn"];
                    out.status = turn["status"].as_str().unwrap_or("unknown").to_owned();
                    out.error = turn["error"]["message"].as_str().map(str::to_owned);
                    return Ok(out);
                }
                _ => {}
            }
        }
    }

    fn send(&mut self, v: &Value) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .context("app-server への入力が閉じています")?;
        writeln!(stdin, "{v}")?;
        stdin.flush()?;
        Ok(())
    }

    fn recv(&mut self, deadline: Instant) -> Result<Option<Value>> {
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        match self.rx.recv_timeout(deadline - now) {
            Ok(v) => Ok(Some(v)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => bail!("app-server が予期せず終了しました"),
        }
    }

    /// The server may send requests of its own (approvals, auth refresh...).
    /// None apply to an unattended read-only turn, but they must be answered
    /// or the server waits forever.
    fn answer_server_request(&mut self, msg: &Value) -> Result<bool> {
        if msg.get("id").is_none() || msg.get("method").is_none() {
            return Ok(false);
        }
        let reply = json!({
            "id": msg["id"],
            "error": {"code": -32601, "message": "codex-switch does not handle this request"}
        });
        self.send(&reply)?;
        Ok(true)
    }
}

impl Drop for AppServer {
    /// Closing stdin lets the server flush and exit on its own; kill only as a
    /// last resort so the thread database is never left half-written.
    fn drop(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
