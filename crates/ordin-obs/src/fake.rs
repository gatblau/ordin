//! A fake OBS: an obs-websocket v5 server on loopback that keeps profiles, scenes, inputs and a
//! recording the way OBS does, records every request it receives, and can be told to refuse or
//! ignore a request type. Its wire contract is obs-websocket's protocol version 5 (RPC version 1),
//! as quoted in ObsRecorderAdapter.md *Example*.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

pub const CHALLENGE: &str = "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=";
pub const SALT: &str = "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=";

/// What OBS holds; the tests set it up and read it back.
#[derive(Debug, Clone)]
pub struct Obs {
    pub obs_version: String,
    pub rpc_version: u64,
    pub password: Option<String>,
    pub profiles: Vec<String>,
    pub current_profile: String,
    pub scenes: Vec<String>,
    pub current_scene: String,
    /// Name → (kind, audio tracks).
    pub inputs: HashMap<String, (String, Value)>,
    pub parameters: Vec<(String, String, String)>,
    /// What real OBS builds its recording output from: the parameters set so far, copied when a
    /// profile is selected (`SetCurrentProfile`, or `CreateProfile`, which selects the new one).
    /// `SetProfileParameter` alone does not change it (ObsRecorderAdapter.md OBS-PRO-1).
    pub loaded_parameters: Vec<(String, String, String)>,
    /// Every profile selected, in order.
    pub selected_profiles: Vec<String>,
    pub recording: bool,
    pub duration_ms: u64,
    pub output_path: PathBuf,
    /// What `GetRecordDirectory` answers; the output path's folder unless a test changes it.
    pub record_directory: PathBuf,
    /// Request type → the failure code OBS answers with.
    pub refuse: HashMap<String, u16>,
    /// Request types OBS never answers.
    pub silent: HashSet<String>,
    /// Every request received, in order: (type, data).
    pub received: Vec<(String, Value)>,
    /// The `authentication` of every `Identify` received.
    pub identified_with: Vec<Option<String>>,
}

impl Obs {
    pub fn new(obs_version: &str, output_path: PathBuf) -> Obs {
        Obs {
            obs_version: obs_version.to_owned(),
            rpc_version: 1,
            password: None,
            profiles: vec!["Untitled".to_owned()],
            current_profile: "Untitled".to_owned(),
            scenes: vec!["Scene".to_owned()],
            current_scene: "Scene".to_owned(),
            inputs: HashMap::new(),
            parameters: Vec::new(),
            loaded_parameters: Vec::new(),
            selected_profiles: Vec::new(),
            recording: false,
            duration_ms: 0,
            record_directory: output_path.parent().map(PathBuf::from).unwrap_or_default(),
            output_path,
            refuse: HashMap::new(),
            silent: HashSet::new(),
            received: Vec::new(),
            identified_with: Vec::new(),
        }
    }

    /// The request types received, in order.
    pub fn types(&self) -> Vec<String> {
        self.received.iter().map(|(t, _)| t.clone()).collect()
    }

    pub fn count(&self, request_type: &str) -> usize {
        self.received
            .iter()
            .filter(|(t, _)| t == request_type)
            .count()
    }
}

/// The fake server; it stops when dropped.
pub struct FakeObs {
    pub port: u16,
    pub obs: Arc<Mutex<Obs>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeObs {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeObs {
    pub async fn start(obs: Obs) -> FakeObs {
        FakeObs::start_on(obs, 0).await
    }

    /// The same on a fixed port (0 picks one): journey 13 serves OBS's own 4455 (PB-MIN-2).
    pub async fn start_on(obs: Obs, port: u16) -> FakeObs {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        let obs = Arc::new(Mutex::new(obs));
        let shared = obs.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(stream, shared.clone()));
            }
        });
        FakeObs { port, obs, task }
    }

    /// Stops accepting connections, as OBS does when it is closed.
    pub fn close(&self) {
        self.task.abort();
    }

    pub fn obs(&self) -> std::sync::MutexGuard<'_, Obs> {
        self.obs.lock().expect("fake obs")
    }
}

/// The authentication string of the obs-websocket protocol, written here independently of the
/// adapter's.
fn expected_authentication(password: &str) -> String {
    let mut first = Sha256::new();
    first.update(password.as_bytes());
    first.update(SALT.as_bytes());
    let secret = STANDARD.encode(first.finalize());
    let mut second = Sha256::new();
    second.update(secret.as_bytes());
    second.update(CHALLENGE.as_bytes());
    STANDARD.encode(second.finalize())
}

#[allow(clippy::result_large_err)]
fn add_subprotocol(_: &Request, mut response: Response) -> Result<Response, ErrorResponse> {
    response.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("obswebsocket.json"),
    );
    Ok(response)
}

async fn serve(stream: tokio::net::TcpStream, obs: Arc<Mutex<Obs>>) {
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, add_subprotocol).await else {
        return;
    };
    let (rpc_version, password) = {
        let o = obs.lock().expect("fake obs");
        (o.rpc_version, o.password.clone())
    };
    let mut hello = json!({ "obsWebSocketVersion": "5.4.2", "rpcVersion": rpc_version });
    if password.is_some() {
        hello["authentication"] = json!({ "challenge": CHALLENGE, "salt": SALT });
    }
    if send(&mut socket, 0, hello).await.is_err() {
        return;
    }
    // Identify.
    let Some(identify) = next(&mut socket).await else {
        return;
    };
    let given = identify["d"]["authentication"].as_str().map(str::to_owned);
    obs.lock()
        .expect("fake obs")
        .identified_with
        .push(given.clone());
    if let Some(password) = password {
        if given.as_deref() != Some(expected_authentication(&password).as_str()) {
            let _ = socket
                .close(Some(CloseFrame {
                    code: CloseCode::from(4009),
                    reason: "Authentication failed.".into(),
                }))
                .await;
            return;
        }
    }
    if send(&mut socket, 2, json!({ "negotiatedRpcVersion": 1 }))
        .await
        .is_err()
    {
        return;
    }
    while let Some(frame) = next(&mut socket).await {
        if frame["op"].as_u64() != Some(6) {
            continue;
        }
        let d = &frame["d"];
        let request_type = d["requestType"].as_str().unwrap_or_default().to_owned();
        let id = d["requestId"].clone();
        let data = d.get("requestData").cloned().unwrap_or(Value::Null);
        let answer = {
            let mut o = obs.lock().expect("fake obs");
            o.received.push((request_type.clone(), data.clone()));
            if o.silent.contains(&request_type) {
                None
            } else if let Some(code) = o.refuse.get(&request_type) {
                Some(Err(*code))
            } else {
                Some(handle(&mut o, &request_type, &data))
            }
        };
        let Some(answer) = answer else {
            continue;
        };
        let mut response = json!({ "requestType": request_type, "requestId": id });
        match answer {
            Ok(body) => {
                response["requestStatus"] = json!({ "result": true, "code": 100 });
                if !body.is_null() {
                    response["responseData"] = body;
                }
            }
            Err(code) => {
                response["requestStatus"] = json!({ "result": false, "code": code });
            }
        }
        if send(&mut socket, 7, response).await.is_err() {
            return;
        }
    }
}

/// OBS's behaviour for each request the adapter sends; 600 is ResourceNotFound, 601
/// ResourceAlreadyExists, 500 OutputRunning, 501 OutputNotRunning.
fn handle(o: &mut Obs, request_type: &str, data: &Value) -> Result<Value, u16> {
    let field = |name: &str| data[name].as_str().unwrap_or_default().to_owned();
    match request_type {
        "GetVersion" => Ok(json!({
            "obsVersion": o.obs_version,
            "obsWebSocketVersion": "5.4.2",
            "rpcVersion": 1,
        })),
        "GetRecordStatus" => Ok(json!({
            "outputActive": o.recording,
            "outputPaused": false,
            "outputDuration": o.duration_ms,
        })),
        "GetProfileList" => Ok(json!({
            "currentProfileName": o.current_profile,
            "profiles": o.profiles,
        })),
        "GetCurrentProgramScene" => Ok(json!({
            "sceneName": o.current_scene,
            "currentProgramSceneName": o.current_scene,
        })),
        "CreateProfile" => {
            let name = field("profileName");
            if o.profiles.contains(&name) {
                return Err(601);
            }
            o.profiles.push(name.clone());
            o.selected_profiles.push(name.clone());
            o.current_profile = name;
            o.loaded_parameters = o.parameters.clone();
            Ok(Value::Null)
        }
        "SetCurrentProfile" => {
            let name = field("profileName");
            if !o.profiles.contains(&name) {
                return Err(600);
            }
            o.selected_profiles.push(name.clone());
            o.current_profile = name;
            o.loaded_parameters = o.parameters.clone();
            Ok(Value::Null)
        }
        "SetProfileParameter" => {
            o.parameters.push((
                field("parameterCategory"),
                field("parameterName"),
                field("parameterValue"),
            ));
            Ok(Value::Null)
        }
        "GetSceneList" => Ok(json!({
            "currentProgramSceneName": o.current_scene,
            "scenes": o.scenes.iter().map(|s| json!({ "sceneName": s })).collect::<Vec<_>>(),
        })),
        "CreateScene" => {
            let name = field("sceneName");
            if o.scenes.contains(&name) {
                return Err(601);
            }
            o.scenes.push(name);
            Ok(Value::Null)
        }
        "CreateInput" => {
            let name = field("inputName");
            if o.inputs.contains_key(&name) || !o.scenes.contains(&field("sceneName")) {
                return Err(601);
            }
            o.inputs.insert(name, (field("inputKind"), Value::Null));
            Ok(json!({ "inputUuid": "00000000-0000-0000-0000-000000000000", "sceneItemId": 1 }))
        }
        "SetInputAudioTracks" => {
            let name = field("inputName");
            let Some(input) = o.inputs.get_mut(&name) else {
                return Err(600);
            };
            input.1 = data["inputAudioTracks"].clone();
            Ok(Value::Null)
        }
        "SetCurrentProgramScene" => {
            let name = field("sceneName");
            if !o.scenes.contains(&name) {
                return Err(600);
            }
            o.current_scene = name;
            Ok(Value::Null)
        }
        "GetRecordDirectory" => Ok(json!({
            "recordDirectory": o.record_directory.to_string_lossy(),
        })),
        "StartRecord" => {
            if o.recording {
                return Err(500);
            }
            o.recording = true;
            Ok(Value::Null)
        }
        "StopRecord" => {
            if !o.recording {
                return Err(501);
            }
            o.recording = false;
            Ok(json!({ "outputPath": o.output_path.to_string_lossy() }))
        }
        _ => Err(204),
    }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn send(socket: &mut Socket, op: u64, d: Value) -> Result<(), ()> {
    socket
        .send(Message::text(json!({ "op": op, "d": d }).to_string()))
        .await
        .map_err(drop)
}

async fn next(socket: &mut Socket) -> Option<Value> {
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => return serde_json::from_str(text.as_str()).ok(),
            Message::Close(_) => return None,
            _ => {}
        }
    }
    None
}
