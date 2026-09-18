use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub struct Control {
    stream: TcpStream,
}

impl Control {
    pub async fn conectar(addr: std::net::SocketAddr, cookie: &[u8]) -> std::io::Result<Self> {
        let stream = timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .map_err(|_| std::io::Error::other("control timeout"))??;
        let mut c = Self { stream };
        let hex = cookie.iter().map(|b| format!("{b:02X}")).collect::<String>();
        let (code, _) = c.cmd(&format!("AUTHENTICATE {hex}")).await?;
        if code != 250 {
            return Err(std::io::Error::other("tor control auth failed"));
        }
        Ok(c)
    }

    pub async fn cmd(&mut self, line: &str) -> std::io::Result<(u16, Vec<String>)> {
        self.stream.write_all(line.as_bytes()).await?;
        self.stream.write_all(b"\r\n").await?;
        self.stream.flush().await?;
        let mut r = BufReader::new(&mut self.stream);
        let mut lines = Vec::new();
        let mut last;
        loop {
            let mut s = String::new();
            let n = r.read_line(&mut s).await?;
            if n == 0 {
                return Err(std::io::Error::other("control closed"));
            }
            let s = s.trim_end_matches(['\r', '\n']).to_string();
            if s.len() < 4 {
                continue;
            }
            let code: u16 = s[..3]
                .parse()
                .map_err(|_| std::io::Error::other("bad control code"))?;
            last = code;
            let sep = s.as_bytes()[3];
            lines.push(s[4..].to_string());
            if sep == b' ' {
                break;
            }
        }
        Ok((last, lines))
    }

    pub async fn getinfo(&mut self, key: &str) -> std::io::Result<String> {
        let (code, lines) = self.cmd(&format!("GETINFO {key}")).await?;
        if code != 250 {
            return Err(std::io::Error::other(format!("GETINFO {key} -> {code}")));
        }
        let prefix = format!("{key}=");
        for l in lines {
            if let Some(rest) = l.strip_prefix(&prefix) {
                return Ok(rest.trim_matches('"').to_string());
            }
        }
        Err(std::io::Error::other("GETINFO empty"))
    }

    pub async fn add_onion(&mut self, key: &str, virt: u16, local: u16) -> std::io::Result<String> {
        let spec = if key == "NEW" {
            "NEW:ED25519-V3".to_string()
        } else {
            format!("ED25519-V3:{key}")
        };
        let cmd = format!(
            "ADD_ONION {spec} Flags=DiscardPK,Detach Port={virt},127.0.0.1:{local}"
        );
        let (code, lines) = self.cmd(&cmd).await?;
        if code != 250 {
            return Err(std::io::Error::other(format!(
                "ADD_ONION {code}: {}",
                lines.join(" / ")
            )));
        }
        for l in lines {
            if let Some(id) = l.strip_prefix("ServiceID=") {
                return Ok(format!("{id}.onion"));
            }
        }
        Err(std::io::Error::other("ADD_ONION without ServiceID"))
    }
}

pub fn parse_progress(phase: &str) -> Option<u32> {
    let marker = "PROGRESS=";
    let i = phase.find(marker)?;
    let rest = &phase[i + marker.len()..];
    let n: u32 = rest
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_progreso() {
        assert_eq!(
            parse_progress(
                "NOTICE BOOTSTRAP PROGRESS=85 TAG=loading_keys SUMMARY=\"Loading\""
            ),
            Some(85)
        );
        assert_eq!(
            parse_progress("NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\""),
            Some(100)
        );
    }
}
