// A client of the Girovo API (https://developer.girovo.de): Girovo's named
// pipe on Windows, its Unix socket on Linux. JSON-RPC 2.0, one message per
// line; a connection opens with girovo.hello, which names the permissions
// this program asks for. Girovo shows its dialog for any it does not have
// yet and answers with everything allowed so far.
//
// Girovo finds out by itself who is at the other end (the process, its file,
// the file's signature); nothing this program says about itself counts for
// that. Signed with the MT-Media certificate, Girovo CLI shows up as
// "Girovo CLI" by MT-Media.
//
// One request at a time: write a line, read lines until the answer with that
// id comes. Notifications that arrive meanwhile wait in a queue for
// `notification()` (watch, a transfer's outcome).

use std::collections::{BTreeSet, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

/// Girovo's lines stay well under this; a longer one ends the connection.
const MAX_LINE: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum Error {
  /// No Girovo to reach: not running, Girovo API off, or the connection gone.
  NotRunning,
  /// What listens at Girovo's address runs as another user: not Girovo.
  NotGirovo,
  /// Girovo turned the connection away (too many open, a program it could
  /// not tell, nothing asked in time): its reason and message.
  Refused { reason: String, message: String },
  /// Girovo's answer was an error: its code (data.reason), message and data.
  Rpc { reason: String, message: String, data: Value },
}

impl Error {
  pub fn reason(&self) -> &str {
    match self {
      Error::Rpc { reason, .. } | Error::Refused { reason, .. } => reason,
      Error::NotRunning => "not_running",
      Error::NotGirovo => "not_girovo",
    }
  }
}

/// Where Girovo listens: GIROVO_API_ADDRESS, else this user's pipe or socket.
pub fn default_address() -> io::Result<String> {
  if let Some(a) = std::env::var_os("GIROVO_API_ADDRESS").filter(|a| !a.is_empty()) {
    return Ok(a.to_string_lossy().into_owned());
  }
  os::default_address()
}

/// An error line with a null id is Girovo turning the connection away; it closes it right after.
fn refusal(message: &Value) -> Option<Error> {
  if !message.get("id").is_some_and(Value::is_null) {
    return None;
  }
  let error = message.get("error")?;
  let reason = error
    .get("data")
    .and_then(|d| d.get("reason"))
    .and_then(Value::as_str)
    .unwrap_or("internal")
    .to_string();
  let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_string();
  Some(Error::Refused { reason, message })
}

pub struct Girovo {
  reader: BufReader<os::Stream>,
  writer: os::Stream,
  next_id: u64,
  queue: VecDeque<Value>,
  /// What Girovo allows this program, as its last hello said.
  granted: BTreeSet<String>,
  /// Girovo's version, from the hello.
  pub version: String,
  /// Called when a hello takes long (a dialog or the login is open in Girovo): says to look there.
  waiting_hint: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Girovo {
  pub fn connect(address: Option<&str>) -> Result<Girovo, Error> {
    let address = match address {
      Some(a) => a.to_string(),
      None => default_address().map_err(|_| Error::NotRunning)?,
    };
    let stream = os::connect(&address).map_err(|e| match e.kind() {
      io::ErrorKind::PermissionDenied => Error::NotGirovo,
      _ => Error::NotRunning,
    })?;
    let writer = stream.try_clone().map_err(|_| Error::NotRunning)?;
    Ok(Girovo {
      reader: BufReader::new(stream),
      writer,
      next_id: 1,
      queue: VecDeque::new(),
      granted: BTreeSet::new(),
      version: String::new(),
      waiting_hint: None,
    })
  }

  pub fn on_waiting(&mut self, hint: impl Fn() + Send + Sync + 'static) {
    self.waiting_hint = Some(Arc::new(hint));
  }

  pub fn granted(&self) -> &BTreeSet<String> {
    &self.granted
  }

  fn read_message(&mut self) -> Result<Value, Error> {
    loop {
      let mut buf = Vec::new();
      match (&mut self.reader).take(MAX_LINE).read_until(b'\n', &mut buf) {
        Ok(n) if n > 0 && buf.ends_with(b"\n") => {}
        _ => return Err(Error::NotRunning),
      }
      if let Ok(message) = serde_json::from_slice::<Value>(&buf) {
        if let Some(refused) = refusal(&message) {
          return Err(refused);
        }
        return Ok(message);
      }
    }
  }

  /// One request and its answer.
  pub fn request(&mut self, method: &str, params: Value) -> Result<Value, Error> {
    let id = self.next_id;
    self.next_id += 1;
    let mut line = serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
    line.push(b'\n');
    if self.writer.write_all(&line).and_then(|_| self.writer.flush()).is_err() {
      // Turned away at once: Girovo closed its end, but the line saying why may still be there.
      return Err(self.read_message().err().unwrap_or(Error::NotRunning));
    }

    // A hello that takes long waits for the user: Girovo's dialog, or the
    // login when Girovo is locked (it holds every call until then).
    let answered = Arc::new(AtomicBool::new(false));
    if let Some(hint) = self.waiting_hint.clone().filter(|_| method == "girovo.hello") {
      let done = answered.clone();
      std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(900));
        if !done.load(Ordering::SeqCst) {
          hint();
        }
      });
    }
    let result = loop {
      let message = match self.read_message() {
        Ok(m) => m,
        Err(e) => break Err(e),
      };
      // A notification: kept for notification().
      let Some(got) = message.get("id") else {
        self.queue.push_back(message);
        continue;
      };
      if got.as_u64() != Some(id) {
        continue;
      }
      if let Some(error) = message.get("error") {
        let data = error.get("data").cloned().unwrap_or(Value::Null);
        let reason = data.get("reason").and_then(Value::as_str).unwrap_or("internal").to_string();
        let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_string();
        break Err(Error::Rpc { reason, message, data });
      }
      break Ok(message.get("result").cloned().unwrap_or(Value::Null));
    };
    answered.store(true, Ordering::SeqCst);
    result
  }

  /// girovo.hello for these permissions; afterwards `granted()` is what Girovo allows.
  pub fn hello(&mut self, permissions: &[&str], reason: &str) -> Result<(), Error> {
    let params = json!({ "app": "Girovo CLI", "version": env!("CARGO_PKG_VERSION"), "permissions": permissions, "reason": reason });
    let result = self.request("girovo.hello", params)?;
    self.granted = result
      .get("granted")
      .and_then(Value::as_array)
      .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
      .unwrap_or_default();
    self.version = result.pointer("/girovo/version").and_then(Value::as_str).unwrap_or("").to_string();
    Ok(())
  }

  /// A method. When Girovo answers `not_permitted`, asks for that one permission
  /// (Girovo's dialog opens) and tries once more.
  pub fn call(&mut self, method: &str, params: Value, reason: &str) -> Result<Value, Error> {
    match self.request(method, params.clone()) {
      Err(Error::Rpc { reason: code, data, .. }) if code == "not_permitted" && data.get("permission").and_then(Value::as_str).is_some() => {
        let permission = data["permission"].as_str().unwrap().to_string();
        self.hello(&[permission.as_str()], reason)?;
        if !self.granted.contains(&permission) {
          return Err(Error::Rpc {
            reason: "refused_by_user".into(),
            message: String::new(),
            data: json!({ "permission": permission }),
          });
        }
        self.request(method, params)
      }
      other => other,
    }
  }

  /// The next notification Girovo sends: `{ method, params }`. Err when the connection ends.
  pub fn notification(&mut self) -> Result<Value, Error> {
    if let Some(n) = self.queue.pop_front() {
      return Ok(n);
    }
    loop {
      let message = self.read_message()?;
      if message.get("id").is_none() && message.get("method").is_some() {
        return Ok(message);
      }
    }
  }
}

#[cfg(windows)]
mod os {
  use std::ffi::c_void;
  use std::io;
  use std::time::{Duration, Instant};

  use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
  use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT};
  use windows::Win32::Security::{GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser};
  use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
  use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
  use windows::core::PWSTR;

  pub type Stream = std::fs::File;

  /// All of the pipe's instances are taken for a moment: Girovo puts a fresh one there right after.
  const ERROR_PIPE_BUSY: i32 = 231;
  /// The server may learn who connects but never act as this user.
  const SECURITY_IDENTIFICATION: u32 = 1 << 16;

  /// `\\.\pipe\girovo-api-<SID>`, as Girovo names it.
  pub fn default_address() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\girovo-api-{}", user_sid()?))
  }

  fn user_sid() -> io::Result<String> {
    unsafe { user_sid_of(GetCurrentProcess()) }
  }

  /// The user a process runs as, as an SID string.
  unsafe fn user_sid_of(process: HANDLE) -> io::Result<String> {
    unsafe {
      let mut token = HANDLE::default();
      OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(io::Error::other)?;
      let mut len = 0u32;
      let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
      let mut buf = vec![0u64; (len as usize).div_ceil(8)];
      let read = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len);
      let _ = CloseHandle(token);
      read.map_err(io::Error::other)?;
      let user = &*(buf.as_ptr() as *const TOKEN_USER);
      let mut text = PWSTR::null();
      ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(io::Error::other)?;
      let sid = text.to_string().map_err(io::Error::other);
      let _ = LocalFree(Some(HLOCAL(text.0 as *mut c_void)));
      sid
    }
  }

  pub fn connect(address: &str) -> io::Result<Stream> {
    use std::os::windows::fs::OpenOptionsExt;
    let start = Instant::now();
    loop {
      let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(address);
      match opened {
        Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && start.elapsed() < Duration::from_secs(2) => {
          std::thread::sleep(Duration::from_millis(20));
        }
        other => return other.and_then(this_users),
      }
    }
  }

  /// Another Windows user could make a pipe of Girovo's name first and read
  /// what this program sends. Only a server that runs as this user is Girovo.
  /// An elevated Girovo's token may be closed to us; then the pipe's owner
  /// decides, which Girovo sets to its user.
  fn this_users(stream: Stream) -> io::Result<Stream> {
    use std::os::windows::io::AsRawHandle;
    let not_girovo = || io::Error::new(io::ErrorKind::PermissionDenied, "the pipe's server runs as another user");
    let mut pid = 0u32;
    unsafe { GetNamedPipeServerProcessId(HANDLE(stream.as_raw_handle()), &mut pid) }.map_err(io::Error::other)?;
    let theirs = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok().and_then(|server| {
      let sid = unsafe { user_sid_of(server) };
      unsafe {
        let _ = CloseHandle(server);
      }
      sid.ok()
    });
    let theirs = match theirs {
      Some(sid) => sid,
      None => pipe_owner(&stream).map_err(|_| not_girovo())?,
    };
    if theirs != user_sid()? {
      return Err(not_girovo());
    }
    Ok(stream)
  }

  fn pipe_owner(stream: &Stream) -> io::Result<String> {
    use std::os::windows::io::AsRawHandle;
    unsafe {
      let mut owner = PSID::default();
      let mut descriptor = PSECURITY_DESCRIPTOR::default();
      GetSecurityInfo(
        HANDLE(stream.as_raw_handle()),
        SE_KERNEL_OBJECT,
        OWNER_SECURITY_INFORMATION,
        Some(&mut owner),
        None,
        None,
        None,
        Some(&mut descriptor),
      )
      .ok()
      .map_err(io::Error::other)?;
      let mut text = PWSTR::null();
      let converted = ConvertSidToStringSidW(owner, &mut text).map_err(io::Error::other);
      let sid = converted.and_then(|_| text.to_string().map_err(io::Error::other));
      if !text.is_null() {
        let _ = LocalFree(Some(HLOCAL(text.0 as *mut c_void)));
      }
      let _ = LocalFree(Some(HLOCAL(descriptor.0)));
      sid
    }
  }
}

#[cfg(unix)]
mod os {
  use std::io;
  use std::path::PathBuf;

  pub type Stream = std::os::unix::net::UnixStream;

  /// `$XDG_RUNTIME_DIR/girovo-api.sock`, else in Girovo's data folder (~/.config/Girovo), as Girovo does.
  pub fn default_address() -> io::Result<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
      .filter(|d| !d.is_empty())
      .map(PathBuf::from)
      .filter(|d| d.is_dir())
      .or_else(|| {
        let config = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()).map(PathBuf::from);
        config
          .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
          .map(|c| c.join("Girovo"))
      })
      .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no runtime or home folder"))?;
    Ok(dir.join("girovo-api.sock").to_string_lossy().into_owned())
  }

  pub fn connect(address: &str) -> io::Result<Stream> {
    let stream = Stream::connect(address)?;
    #[cfg(target_os = "linux")]
    this_users(&stream)?;
    Ok(stream)
  }

  /// Only a server that runs as this user is Girovo.
  #[cfg(target_os = "linux")]
  fn this_users(stream: &Stream) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let read = unsafe {
      libc::getsockopt(
        stream.as_raw_fd(),
        libc::SOL_SOCKET,
        libc::SO_PEERCRED,
        &mut cred as *mut libc::ucred as *mut libc::c_void,
        &mut len,
      )
    };
    if read != 0 || cred.uid != unsafe { libc::getuid() } {
      return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the socket's server runs as another user"));
    }
    Ok(())
  }
}
