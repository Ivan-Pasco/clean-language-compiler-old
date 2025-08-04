// Simple HTTP Client Implementation for Clean Language (std only)
use crate::error::CompilerError;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::OnceLock;

pub struct HttpClient;

/// HTTP response structure
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: String,
}

impl HttpClient {
    /// Create a new HTTP client with default settings
    pub fn new() -> Self {
        HttpClient
    }

    /// Make an HTTP GET request
    pub fn get(&self, url: &str) -> Result<HttpResponse, CompilerError> {
        println!("🌐 [HTTP GET] Making real request to: {url}");

        // Parse URL (simple implementation)
        let (host, path) = self.parse_url(url)?;

        // Create HTTP request
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Clean-Language/1.0\r\nConnection: close\r\n\r\n");

        self.send_request(&host, &request)
    }

    /// Make an HTTP POST request
    pub fn post(&self, url: &str, body: &str) -> Result<HttpResponse, CompilerError> {
        println!("🌐 [HTTP POST] Making real request to: {url}");

        let (host, path) = self.parse_url(url)?;

        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Clean-Language/1.0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path, host, body.len(), body
        );

        self.send_request(&host, &request)
    }

    /// Make an HTTP PUT request
    pub fn put(&self, url: &str, body: &str) -> Result<HttpResponse, CompilerError> {
        println!("🌐 [HTTP PUT] Making real request to: {url}");

        let (host, path) = self.parse_url(url)?;

        let request = format!(
            "PUT {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Clean-Language/1.0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path, host, body.len(), body
        );

        self.send_request(&host, &request)
    }

    /// Make an HTTP PATCH request
    pub fn patch(&self, url: &str, body: &str) -> Result<HttpResponse, CompilerError> {
        println!("🌐 [HTTP PATCH] Making real request to: {url}");

        let (host, path) = self.parse_url(url)?;

        let request = format!(
            "PATCH {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Clean-Language/1.0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path, host, body.len(), body
        );

        self.send_request(&host, &request)
    }

    /// Make an HTTP DELETE request
    pub fn delete(&self, url: &str) -> Result<HttpResponse, CompilerError> {
        println!("🌐 [HTTP DELETE] Making real request to: {url}");

        let (host, path) = self.parse_url(url)?;

        let request = format!("DELETE {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Clean-Language/1.0\r\nConnection: close\r\n\r\n");

        self.send_request(&host, &request)
    }

    fn parse_url(&self, url: &str) -> Result<(String, String), CompilerError> {
        // Simple URL parsing for http://host/path or https://host/path
        let url = url.trim();

        let without_protocol = if url.starts_with("https://") {
            &url[8..]
        } else if url.starts_with("http://") {
            &url[7..]
        } else {
            return Err(CompilerError::runtime_error(
                format!("Unsupported URL scheme: {url}"),
                None,
                None,
            ));
        };

        let parts: Vec<&str> = without_protocol.splitn(2, '/').collect();
        let host = parts[0].to_string();
        let path = if parts.len() > 1 {
            format!("/{}", parts[1])
        } else {
            "/".to_string()
        };

        Ok((host, path))
    }

    fn send_request(&self, host: &str, request: &str) -> Result<HttpResponse, CompilerError> {
        // Connect to server (port 80 for HTTP, 443 for HTTPS not supported in this simple implementation)
        let address = format!("{host}:80");

        match TcpStream::connect(&address) {
            Ok(mut stream) => {
                // Send request
                if let Err(e) = stream.write_all(request.as_bytes()) {
                    return Err(CompilerError::runtime_error(
                        format!("Failed to send HTTP request: {e}"),
                        None,
                        None,
                    ));
                }

                // Read response
                let mut response = String::new();
                if let Err(e) = stream.read_to_string(&mut response) {
                    return Err(CompilerError::runtime_error(
                        format!("Failed to read HTTP response: {e}"),
                        None,
                        None,
                    ));
                }

                // Parse response
                self.parse_response(&response)
            }
            Err(e) => Err(CompilerError::runtime_error(
                format!("Failed to connect to {host}: {e}"),
                None,
                None,
            )),
        }
    }

    fn parse_response(&self, response: &str) -> Result<HttpResponse, CompilerError> {
        let lines: Vec<&str> = response.lines().collect();

        if lines.is_empty() {
            return Err(CompilerError::runtime_error(
                "Empty HTTP response".to_string(),
                None,
                None,
            ));
        }

        // Parse status line (e.g., "HTTP/1.1 200 OK")
        let status_line = lines[0];
        let status_parts: Vec<&str> = status_line.split_whitespace().collect();

        let status_code = if status_parts.len() >= 2 {
            status_parts[1].parse::<u16>().unwrap_or(500)
        } else {
            500
        };

        // Find empty line separating headers from body
        let mut body_start = 0;
        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                body_start = i + 1;
                break;
            }
        }

        // Extract body
        let body = if body_start < lines.len() {
            lines[body_start..].join("\n")
        } else {
            String::new()
        };

        println!(
            "✅ [HTTP] Response received: {} bytes, status {status_code}",
            body.len()
        );

        Ok(HttpResponse { status_code, body })
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Global HTTP client instance
static HTTP_CLIENT: OnceLock<HttpClient> = OnceLock::new();

/// Initialize the global HTTP client
pub fn init_http_client() {
    HTTP_CLIENT.get_or_init(HttpClient::new);
}

/// Get the global HTTP client
pub fn get_http_client() -> &'static HttpClient {
    HTTP_CLIENT.get().expect("HTTP client not initialized")
}

/// Convert HttpResponse to a string for Clean Language runtime
pub fn response_to_string(response: &HttpResponse) -> String {
    response.body.clone()
}

/// Convert HttpResponse to a status code for Clean Language runtime
pub fn response_to_status_code(response: &HttpResponse) -> i32 {
    response.status_code as i32
}
