//! Small HTTP/1 client for local Lily and audio-sink requests.
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub(super) const HEADER_LIMIT: usize = 8192;
pub(super) const BODY_LIMIT: usize = 1024;

fn remaining(deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!("loopback HTTP request deadline expired")
    }
    Ok(remaining.min(Duration::from_secs(1)))
}

pub(super) fn exchange(port: u16, mut request: &[u8], deadline: Instant) -> Result<Vec<u8>> {
    let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&endpoint, remaining(deadline)?)
        .with_context(|| format!("connect {endpoint}"))?;
    while !request.is_empty() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        let written = stream.write(request)?;
        if written == 0 {
            bail!("loopback HTTP request write made no progress")
        }
        request = &request[written..];
    }

    let mut response = Vec::new();
    let mut expected = None;
    let mut buffer = [0u8; 2048];
    loop {
        if let Some((body_start, body_len)) = expected {
            if response.len() == body_start + body_len {
                response.drain(..body_start);
                return Ok(response);
            }
            if response.len() > body_start + body_len {
                bail!("loopback HTTP response has bytes beyond Content-Length")
            }
        }
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            bail!("loopback HTTP response ended before its complete body")
        }
        if response.len() + count > HEADER_LIMIT + BODY_LIMIT {
            bail!("loopback HTTP response is too large")
        }
        response.extend_from_slice(&buffer[..count]);
        if expected.is_none() {
            if let Some(end) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let body_start = end + 4;
                if body_start > HEADER_LIMIT {
                    bail!("loopback HTTP response headers are too large")
                }
                expected = Some((body_start, response_body_length(&response[..body_start])?));
            } else if response.len() >= HEADER_LIMIT {
                bail!("loopback HTTP response headers are too large")
            }
        }
    }
}

pub(super) fn response_body_length(header: &[u8]) -> Result<usize> {
    if header.len() > HEADER_LIMIT {
        bail!("loopback HTTP response headers are too large")
    }
    let header = std::str::from_utf8(header)?;
    let mut lines = header.split("\r\n");
    let mut status = lines.next().unwrap_or_default().split_whitespace();
    if !matches!(status.next(), Some("HTTP/1.1" | "HTTP/1.0")) || status.next() != Some("200") {
        bail!("loopback HTTP endpoint did not return HTTP 200")
    }
    let mut length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("malformed loopback HTTP header"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            bail!("loopback HTTP response must not use transfer encoding")
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                bail!("duplicate loopback HTTP Content-Length")
            }
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length =
        length.ok_or_else(|| anyhow::anyhow!("loopback HTTP response has no Content-Length"))?;
    if length > BODY_LIMIT {
        bail!("loopback HTTP JSON body is too large")
    }
    Ok(length)
}
