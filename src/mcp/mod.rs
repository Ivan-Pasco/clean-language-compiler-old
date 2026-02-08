/*!
 * Model Context Protocol (MCP) Server
 *
 * Provides an MCP server interface for AI agents to interact with the Clean Language compiler.
 * Supports JSON-RPC 2.0 protocol for tool-based interactions.
 */

pub mod protocol;
pub mod server;

pub use server::run_mcp_server;
