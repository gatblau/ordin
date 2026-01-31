//! A hand-written loopback stand-in for `GET /v1/events` (and the other Remote API routes the
//! supervisor meets), honouring RemoteApi.md *Routes*: status 200 `text/event-stream` carrying
//! `Ordin-Api-Version`, frames `id:` / `event:` / `data:`, a `: heartbeat` comment, `Last-Event-ID`
//! read from the request, a first `reset` frame for a first stream, chunked transfer coding. Every
//! connection plays the next scripted session; when the script is used up the listener is dropped, so
//! later connections are refused.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
pub enum Step {
    /// `id:`, `event:`, `data:` and a blank line.
    Frame {
        id: String,
        event: String,
        data: String,
    },
    /// `: heartbeat` and a blank line.
    Heartbeat,
    /// Raw bytes written as one chunk.
    Raw(String),
    Wait(Duration),
    /// Hold the connection open until the client closes it.
    Hold,
    /// End the chunked body and close.
    Close,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub status: u16,
    pub version: Option<&'static str>,
    pub body: Option<String>,
    /// Read the request, then close without answering a byte.
    pub drop_conn: bool,
    pub steps: Vec<Step>,
}

impl Session {
    pub fn ok(steps: Vec<Step>) -> Session {
        Session {
            status: 200,
            version: Some("1.0"),
            body: None,
            drop_conn: false,
            steps,
        }
    }

    pub fn dropped() -> Session {
        Session {
            status: 0,
            version: None,
            body: None,
            drop_conn: true,
            steps: vec![],
        }
    }

    pub fn status(status: u16, body: &str) -> Session {
        Session {
            status,
            version: Some("1.0"),
            body: Some(body.to_owned()),
            drop_conn: false,
            steps: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub last_event_id: Option<String>,
    pub authorization: Option<String>,
    pub body: String,
}

pub struct SseStandIn {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    pub task: tokio::task::JoinHandle<()>,
}

impl Drop for SseStandIn {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SseStandIn {
    pub async fn start(sessions: Vec<Session>) -> SseStandIn {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let task = tokio::spawn(async move {
            for session in sessions {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let _ = serve(stream, session, log).await;
                });
            }
            // The script is used up: dropping the listener refuses every later connection.
            drop(listener);
        });
        SseStandIn { port, seen, task }
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub fn events_requests(&self) -> Vec<Seen> {
        self.requests()
            .into_iter()
            .filter(|r| r.path == "/v1/events")
            .collect()
    }
}

pub fn frame(seq: u64, kind: &str, data: String) -> Step {
    Step::Frame {
        id: seq.to_string(),
        event: kind.to_owned(),
        data,
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let first = lines.next()?.to_owned();
    let mut parts = first.split(' ');
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    let mut last_event_id = None;
    let mut authorization = None;
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "last-event-id" => last_event_id = Some(value.trim().to_owned()),
                "authorization" => authorization = Some(value.trim().to_owned()),
                "content-length" => length = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    while buf.len() < head_end + length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    Some(Seen {
        method,
        path,
        last_event_id,
        authorization,
        body,
    })
}

async fn chunk(stream: &mut TcpStream, text: &str) -> std::io::Result<()> {
    stream
        .write_all(format!("{:x}\r\n{}\r\n", text.len(), text).as_bytes())
        .await?;
    stream.flush().await
}

async fn serve(
    mut stream: TcpStream,
    session: Session,
    log: Arc<Mutex<Vec<Seen>>>,
) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await else {
        return Ok(());
    };
    log.lock().unwrap().push(request);
    if session.drop_conn {
        return Ok(());
    }
    let version = session
        .version
        .map(|v| format!("Ordin-Api-Version: {v}\r\n"))
        .unwrap_or_default();
    if let Some(body) = &session.body {
        let head = format!(
            "HTTP/1.1 {} X\r\n{version}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            session.status,
            body.len()
        );
        stream.write_all(head.as_bytes()).await?;
        return stream.shutdown().await;
    }
    let head = format!(
        "HTTP/1.1 {} X\r\n{version}Content-Type: text/event-stream\r\nCache-Control: no-store\r\nTransfer-Encoding: chunked\r\n\r\n",
        session.status
    );
    stream.write_all(head.as_bytes()).await?;
    for step in session.steps {
        match step {
            Step::Frame { id, event, data } => {
                chunk(
                    &mut stream,
                    &format!("id: {id}\nevent: {event}\ndata: {data}\n\n"),
                )
                .await?
            }
            Step::Heartbeat => chunk(&mut stream, ": heartbeat\n\n").await?,
            Step::Raw(text) => chunk(&mut stream, &text).await?,
            Step::Wait(d) => tokio::time::sleep(d).await,
            Step::Hold => {
                let mut sink = [0u8; 64];
                while stream.read(&mut sink).await.unwrap_or(0) > 0 {}
                return Ok(());
            }
            Step::Close => {
                stream.write_all(b"0\r\n\r\n").await?;
                return stream.shutdown().await;
            }
        }
    }
    Ok(())
}
