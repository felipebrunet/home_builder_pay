//! Throwaway localhost UI that shells out to `hbp --yes`.

use std::path::PathBuf;
use std::process::Stdio;

use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

const INDEX: &str = include_str!("index.html");

#[derive(Deserialize)]
struct Req {
    dir: String,
    passphrase: Option<String>,
    args: Vec<String>,
}

#[derive(Serialize)]
struct Resp {
    stdout: String,
    stderr: String,
    error: Option<String>,
}

fn hbp_bin() -> PathBuf {
    if let Ok(p) = std::env::var("HBP") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("hbp");
            if cand.exists() {
                return cand;
            }
        }
    }
    PathBuf::from("hbp")
}

async fn api(Json(req): Json<Req>) -> Json<Resp> {
    if req.args.is_empty() {
        return Json(Resp {
            stdout: String::new(),
            stderr: String::new(),
            error: Some("empty args".into()),
        });
    }
    let mut cmd = Command::new(hbp_bin());
    cmd.arg("--dir").arg(&req.dir).arg("--yes");
    if let Some(p) = req.passphrase.as_deref() {
        if !p.is_empty() {
            cmd.arg("--passphrase").arg(p);
        }
    }
    cmd.args(&req.args);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    match cmd.output().await {
        Ok(o) => Json(Resp {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            error: if o.status.success() {
                None
            } else {
                Some(format!("exit {}", o.status))
            },
        }),
        Err(e) => Json(Resp {
            stdout: String::new(),
            stderr: String::new(),
            error: Some(e.to_string()),
        }),
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/api/hbp", post(api));
    let addr = "127.0.0.1:3847";
    eprintln!("hbp-ui http://{addr}  (localhost only; test UI)");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}
