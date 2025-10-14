//! HTTP Client Implementation for Clean Language
//!
//! This module provides a robust HTTP client using reqwest with TLS support.
//! Supports HTTP and HTTPS with proper error handling and logging.

use crate::error::CompilerError;
use std::sync::OnceLock;

/// HTTP client wrapper around reqwest
pub struct HttpClient {
    client: reqwest::blocking::Client,
}

/// HTTP response structure
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: String,
}

impl HttpClient {
    /// Create a new HTTP client with default settings
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Clean-Language/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        tracing::debug!("HTTP client initialized with TLS support");

        HttpClient { client }
    }

    /// Make an HTTP GET request
    pub fn get(&self, url: &str) -> Result<HttpResponse, CompilerError> {
        tracing::debug!(url = %url, "HTTP GET request");

        let response = self.client.get(url).send().map_err(|e| {
            CompilerError::runtime_error(format!("HTTP GET request failed: {}", e), None, None)
        })?;

        self.build_response(response)
    }

    /// Make an HTTP POST request
    pub fn post(&self, url: &str, body: &str) -> Result<HttpResponse, CompilerError> {
        tracing::debug!(url = %url, body_len = body.len(), "HTTP POST request");

        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| {
                CompilerError::runtime_error(format!("HTTP POST request failed: {}", e), None, None)
            })?;

        self.build_response(response)
    }

    /// Make an HTTP PUT request
    pub fn put(&self, url: &str, body: &str) -> Result<HttpResponse, CompilerError> {
        tracing::debug!(url = %url, body_len = body.len(), "HTTP PUT request");

        let response = self
            .client
            .put(url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| {
                CompilerError::runtime_error(format!("HTTP PUT request failed: {}", e), None, None)
            })?;

        self.build_response(response)
    }

    /// Make an HTTP PATCH request
    pub fn patch(&self, url: &str, body: &str) -> Result<HttpResponse, CompilerError> {
        tracing::debug!(url = %url, body_len = body.len(), "HTTP PATCH request");

        let response = self
            .client
            .patch(url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| {
                CompilerError::runtime_error(
                    format!("HTTP PATCH request failed: {}", e),
                    None,
                    None,
                )
            })?;

        self.build_response(response)
    }

    /// Make an HTTP DELETE request
    pub fn delete(&self, url: &str) -> Result<HttpResponse, CompilerError> {
        tracing::debug!(url = %url, "HTTP DELETE request");

        let response = self.client.delete(url).send().map_err(|e| {
            CompilerError::runtime_error(format!("HTTP DELETE request failed: {}", e), None, None)
        })?;

        self.build_response(response)
    }

    /// Build HttpResponse from reqwest::Response
    fn build_response(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<HttpResponse, CompilerError> {
        let status_code = response.status().as_u16();

        let body = response.text().map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to read HTTP response body: {}", e),
                None,
                None,
            )
        })?;

        tracing::debug!(
            status = status_code,
            body_len = body.len(),
            "HTTP response received"
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
