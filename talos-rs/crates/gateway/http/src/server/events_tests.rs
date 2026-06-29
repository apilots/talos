use super::*;

#[test]
fn parse_cursor_prefers_query_over_last_event_id() {
    let mut headers = HeaderMap::new();
    headers.insert("last-event-id", "ev_00000000000000000001".parse().unwrap());

    let cursor = parse_cursor(Some("ev_00000000000000000002".to_string()), &headers)
        .unwrap()
        .unwrap();

    assert_eq!(cursor.as_str(), "ev_00000000000000000002");
}
