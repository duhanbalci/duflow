//! Gömülü UI: `ui-dist/index.html` derleme anında binary'ye gömülür; veri JSON'u
//! `__DUFLOW_DATA__` yer tutucusuna basılır. Tek dosya, sunucusuz açılır.

use anyhow::{Context, Result};
use duflow_core::Graph;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;

const TEMPLATE: &str = include_str!("../ui-dist/index.html");
const PLACEHOLDER: &str = "__DUFLOW_DATA__";

pub fn render(data_json: &str) -> String {
    // `</script>` kaçışı: JSON içinde geçerse script'i kapatmasın
    let safe = data_json.replace("</", "<\\/");
    TEMPLATE.replacen(PLACEHOLDER, &safe, 1)
}

/// Her GET'te grafı yeniden yükleyip render eder. `?diff=a..b` overlay.
pub fn serve(dir: &Path, addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr).with_context(|| format!("{addr} dinlenemedi"))?;
    eprintln!("duflow ui: http://{addr}/  (flows: {})", dir.display());
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        // başlıkları tüket
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                break;
            }
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/");
        let diff = path.split_once("diff=").map(|(_, q)| q.split('&').next().unwrap_or("").to_string()).filter(|s| !s.is_empty());
        let body = match Graph::load(dir).map_err(anyhow::Error::from).and_then(|g| crate::export_json(dir, &g, diff.as_deref())) {
            Ok(json) => render(&json),
            Err(e) => format!("<pre style=\"font:14px monospace;padding:2rem\">duflow: {e:#}</pre>"),
        };
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(body.as_bytes());
    }
    Ok(())
}
