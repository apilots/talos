use super::*;

#[test]
fn test_map_mcp_json_rpc_error_known_codes() {
    assert_eq!(
        map_mcp_json_rpc_error("MCP resources/list error -32601: missing"),
        "MCP method not supported: missing"
    );
    assert_eq!(
        map_mcp_json_rpc_error("MCP resources/list error -32602: bad args"),
        "Invalid MCP arguments: bad args"
    );
    assert_eq!(
        map_mcp_json_rpc_error("MCP resources/list error -32603: boom"),
        "MCP server internal error: boom"
    );
}
