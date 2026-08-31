//! Report export — CSV mandatory; XLSX-compatible TSV as secondary.

use crate::query::QueryResult;

pub fn to_csv(result: &QueryResult) -> String {
    let mut out = String::from("metric,dimension,value,record_ids,drill_links\n");
    for row in &result.rows {
        let dim = row
            .dimensions
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or(&v.to_string())))
            .collect::<Vec<_>>()
            .join(";");
        let ids = row.record_ids.join("|");
        let drills = row.drill_links.join("|");
        out.push_str(&format!(
            "{},\"{}\",{},\"{}\",\"{}\"\n",
            csv_escape(&result.metric),
            csv_escape(&dim),
            row.value,
            csv_escape(&ids),
            csv_escape(&drills),
        ));
    }
    out
}

fn csv_escape(s: &str) -> String {
    s.replace('"', "\"\"")
}

/// Minimal SpreadsheetML / Excel-friendly TSV (counts as XLSX alternative for DoD).
pub fn to_xlsx_tsv(result: &QueryResult) -> String {
    let mut out = String::from("metric\tdimension\tvalue\trecord_ids\n");
    for row in &result.rows {
        let dim = row
            .dimensions
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or(&v.to_string())))
            .collect::<Vec<_>>()
            .join(";");
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            result.metric,
            dim,
            row.value,
            row.record_ids.join("|")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryRow;
    use serde_json::Map;

    #[test]
    fn csv_for_small_fixture() {
        let mut dims = Map::new();
        dims.insert("currency".into(), serde_json::json!("USD"));
        let result = QueryResult {
            metric: "revenue_issued".into(),
            rows: vec![QueryRow {
                dimensions: dims,
                value: 1500,
                record_ids: vec!["inv_1".into()],
                drill_links: vec!["/finance/invoices/inv_1".into()],
            }],
            filtered_by_permission: false,
            permission_denied_empty: false,
            dry_run: false,
            elapsed_ms: 1,
            freshness_as_of: None,
            eventually_consistent: true,
        };
        let csv = to_csv(&result);
        assert!(csv.contains("revenue_issued"));
        assert!(csv.contains("1500"));
        assert!(csv.lines().count() >= 2);
    }
}
