//! HTTPS via optional Tor SOCKS5 (`ureq` + `socks-proxy`).

use std::net::SocketAddr;
use std::time::Duration;

pub fn http_agent(socks: Option<SocketAddr>) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(18))
        .user_agent("home_builder_pay/0.1");
    if let Some(addr) = socks {
        if let Ok(p) = ureq::Proxy::new(&format!("socks5://{addr}")) {
            b = b.proxy(p);
        }
    }
    b.build()
}

pub fn get_text(socks: Option<SocketAddr>, url: &str) -> crate::Result<String> {
    let body = http_agent(socks)
        .get(url)
        .call()
        .map_err(|e| crate::Error::msg(e.to_string()))?
        .into_string()
        .map_err(|e| crate::Error::msg(e.to_string()))?;
    Ok(body)
}

pub fn post_text(socks: Option<SocketAddr>, url: &str, body: &str) -> crate::Result<String> {
    let resp = http_agent(socks)
        .post(url)
        .set("Content-Type", "text/plain; charset=utf-8")
        .send_string(body)
        .map_err(|e| crate::Error::msg(e.to_string()))?
        .into_string()
        .map_err(|e| crate::Error::msg(e.to_string()))?;
    Ok(resp)
}

pub fn put_text(socks: Option<SocketAddr>, url: &str, body: &str) -> crate::Result<String> {
    let resp = http_agent(socks)
        .put(url)
        .set("Content-Type", "text/plain; charset=utf-8")
        .send_string(body)
        .map_err(|e| crate::Error::msg(e.to_string()))?
        .into_string()
        .map_err(|e| crate::Error::msg(e.to_string()))?;
    Ok(resp)
}
