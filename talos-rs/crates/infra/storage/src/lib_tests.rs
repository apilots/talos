use super::*;

#[tokio::test]
async fn test_row_get() {
    let row = Row::new(
        vec!["id".to_string(), "name".to_string()],
        vec![Value::Number(1.into()), Value::String("test".to_string())],
    );

    assert_eq!(row.get_i64("id"), Some(1));
    assert_eq!(row.get_string("name"), Some("test".to_string()));
    assert_eq!(row.get("unknown"), None);
}
