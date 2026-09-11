//! Port of `run.py`'s report serving: a local static HTTP server plus the
//! optional Cloudflare quick tunnel used by `--remote`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// `detect_environment()` — capability probe for the CLI banner.
pub struct Environment {
    pub has_browser: bool,
    pub has_cloudflared: bool,
}

pub fn detect_environment() -> Environment {
    Environment {
        has_browser: cfg!(target_os = "macos") || std::env::var("DISPLAY").is_ok(),
        has_cloudflared: which("cloudflared"),
    }
}

fn which(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(bin);
        candidate.is_file()
    })
}

/// Content type by extension, matching `SimpleHTTPRequestHandler.guess_type`.
fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// A minimal static file server rooted at the report's directory.
///
/// Mirrors `serve_report`: the report directory becomes the document root, so a
/// request for `/<filename>` returns the standalone report and sibling assets
/// (avatars, share cards) resolve too.
pub struct ReportServer {
    pub port: u16,
    root: PathBuf,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ReportServer {
    pub fn start(report_path: &Path, port: u16) -> std::io::Result<ReportServer> {
        if port == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid HTTP port: {}", port),
            ));
        }
        let root = report_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;

        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = shutdown.clone();
        let serve_root = root.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let root = serve_root.clone();
                        std::thread::spawn(move || {
                            let _ = handle_request(stream, &root);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });

        let filename = report_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        println!("\n📡 本地 HTTP 服务已启动:");
        println!("   http://localhost:{}/{}", port, filename);

        Ok(ReportServer {
            port,
            root,
            shutdown,
            handle: Some(handle),
        })
    }

    /// Block until interrupted, then shut the server down.
    pub fn wait_until_interrupted(self) {
        // The Rust CLI has no cross-platform SIGINT handler here; keep the
        // process alive while the thread serves, matching upstream's blocking wait.
        println!("   ⏹  按 Ctrl+C 停止服务");
        while !self.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn stop(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for ReportServer {
    fn drop(&mut self) {
        self.stop();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn handle_request(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // drain headers
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("GET");
    let raw_path = parts.next().unwrap_or("/");
    let rel = raw_path.split('?').next().unwrap_or("/").trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };

    // Reject traversal outside the document root.
    let candidate = root.join(rel);
    let resolved = candidate
        .canonicalize()
        .unwrap_or_else(|_| root.join(rel));
    let ok = resolved.starts_with(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()));

    if !ok || !resolved.is_file() {
        let body = b"404 Not Found";
        let _ = stream.write_all(
            format!(
                "HTTP/1.0 404 Not Found\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        );
        let _ = stream.write_all(body);
        return Ok(());
    }

    let mut file = std::fs::File::open(&resolved)?;
    let mut body = Vec::new();
    file.read_to_end(&mut body)?;
    let header = format!(
        "HTTP/1.0 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        content_type(&resolved),
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

/// `start_cloudflare_tunnel` — returns the public URL and the tunnel child.
///
/// Without `cloudflared` (and without `install`) upstream only prints how to
/// install it and never modifies the system; that behaviour is preserved.
pub fn start_cloudflare_tunnel(port: u16, install: bool) -> (Option<String>, Option<Child>) {
    if !which("cloudflared") {
        println!("\n⚠️  未检测到 cloudflared。");
        if !install {
            println!("   未执行自动安装；如需自动安装，请加 --install-cloudflared 后重试。");
            println!("   手动安装: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/");
            return (None, None);
        }
        println!("   --install-cloudflared 已开启，正在尝试安装...");
        let status = if cfg!(target_os = "macos") {
            Command::new("brew").args(["install", "cloudflared"]).status()
        } else if cfg!(target_os = "windows") {
            println!("   请手动安装: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/");
            println!("   或: winget install Cloudflare.cloudflared");
            return (None, None);
        } else {
            Command::new("bash")
                .arg("-c")
                .arg("curl -fsSL https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 -o /tmp/cloudflared && chmod +x /tmp/cloudflared && sudo mv /tmp/cloudflared /usr/local/bin/")
                .status()
        };
        if status.map(|s| !s.success()).unwrap_or(true) || !which("cloudflared") {
            println!("   ❌ cloudflared 安装失败，跳过远程映射");
            return (None, None);
        }
        println!("   ✓ cloudflared 安装成功");
    }

    println!("\n🌐 正在启动 Cloudflare Tunnel (端口 {})...", port);
    let child = Command::new("cloudflared")
        .args(["tunnel", "--url", &format!("http://127.0.0.1:{}", port)])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            println!("   ⚠️ 无法启动 cloudflared: {}", e);
            return (None, None);
        }
    };

    let mut public_url = None;
    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        let deadline = Instant::now() + Duration::from_secs(30);
        for line in reader.lines() {
            if Instant::now() > deadline {
                break;
            }
            let Ok(line) = line else { break };
            if let Some(url) = extract_tunnel_url(&line) {
                public_url = Some(url);
                break;
            }
        }
    }

    match public_url {
        Some(url) => {
            println!("   ✅ 公网地址: {}", url);
            println!("   📱 手机扫码或发送链接即可查看报告");
            println!("   ⚠️  该链接是公网 bearer URL，仅分享给可信对象");
            println!("   ⏹  按 Ctrl+C 停止服务");
            (Some(url), Some(child))
        }
        None => {
            println!("   ⚠️  Tunnel 启动中... 请检查 cloudflared 输出");
            let _ = child.kill();
            (None, None)
        }
    }
}

/// Extract `https://<name>.trycloudflare.com` from a cloudflared log line.
pub fn extract_tunnel_url(line: &str) -> Option<String> {
    if !line.contains("trycloudflare.com") && !line.contains("cfargotunnel.com") {
        return None;
    }
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '/' | ':')))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let url = &rest[..end];
    if url.contains("trycloudflare.com") {
        Some(url.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_public_tunnel_url_from_cloudflared_log() {
        let line = "2026-09-11T10:00:00Z INF |  https://random-words-here.trycloudflare.com  |";
        assert_eq!(
            extract_tunnel_url(line).as_deref(),
            Some("https://random-words-here.trycloudflare.com")
        );
        assert_eq!(extract_tunnel_url("INF no url here"), None);
        assert_eq!(
            extract_tunnel_url("https://x.cfargotunnel.com/abc"),
            None,
            "non-trycloudflare hosts are ignored like upstream"
        );
    }

    #[test]
    fn content_types_cover_the_report_artifacts() {
        assert_eq!(content_type(Path::new("a.html")), "text/html");
        assert_eq!(content_type(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("a.png")), "image/png");
        assert_eq!(content_type(Path::new("a.unknown")), "application/octet-stream");
    }

    #[test]
    fn serves_the_report_over_http() {
        let dir = std::env::temp_dir().join("uzi_serve_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let report = dir.join("full-report-standalone.html");
        std::fs::write(&report, "<html><body>水晶光电</body></html>").unwrap();

        // Port 0 is rejected like upstream; pick a high fixed port for the test.
        assert!(ReportServer::start(&report, 0).is_err());

        let server = ReportServer::start(&report, 18899).unwrap();
        let response = std::net::TcpStream::connect(("127.0.0.1", 18899))
            .and_then(|mut s| {
                use std::io::Write as _;
                s.write_all(b"GET /full-report-standalone.html HTTP/1.0\r\n\r\n")?;
                let mut buf = String::new();
                use std::io::Read as _;
                s.read_to_string(&mut buf)?;
                Ok(buf)
            })
            .expect("request failed");
        assert!(response.starts_with("HTTP/1.0 200 OK"));
        assert!(response.contains("Content-Type: text/html"));
        assert!(response.contains("水晶光电"));
        drop(server);

        // missing file -> 404
        let server = ReportServer::start(&report, 18898).unwrap();
        let response = std::net::TcpStream::connect(("127.0.0.1", 18898))
            .and_then(|mut s| {
                use std::io::Write as _;
                s.write_all(b"GET /nope.html HTTP/1.0\r\n\r\n")?;
                let mut buf = String::new();
                use std::io::Read as _;
                s.read_to_string(&mut buf)?;
                Ok(buf)
            })
            .unwrap();
        assert!(response.starts_with("HTTP/1.0 404"));
        drop(server);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
