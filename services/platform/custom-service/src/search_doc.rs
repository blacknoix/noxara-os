//! Search document path for custom records (indexer hook without requiring
//! a live search consumer in every test).

use serde_json::{json, Value};

/// Build a search document payload that the search-indexer can ingest from
/// `custom.{entity}.created|updated` events.
pub fn search_document(
    org_public: &str,
    entity_slug: &str,
    record_public_id: &str,
    values: &Value,
    search_text: &str,
) -> Value {
    json!({
        "doc_type": format!("custom:{entity_slug}"),
        "org_id": org_public,
        "id": record_public_id,
        "title": search_text,
        "body": values,
        "permission": format!("custom.{entity_slug}.read"),
    })
}

/// Flatten text-ish values for `search_text` / ranking.
pub fn build_search_text(values: &serde_json::Map<String, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in values {
        match v {
            Value::String(s) => parts.push(format!("{k}:{s}")),
            Value::Number(n) => parts.push(format!("{k}:{n}")),
            Value::Bool(b) => parts.push(format!("{k}:{b}")),
            _ => {}
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_doc_carries_permission() {
        let doc = search_document(
            "org_x",
            "widget",
            "cusrec_1",
            &json!({"name": "A"}),
            "name:A",
        );
        assert_eq!(doc["permission"], "custom.widget.read");
        assert_eq!(doc["doc_type"], "custom:widget");
    }
}
