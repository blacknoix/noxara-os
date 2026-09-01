//! Deep link parsing for `companyos://record/{id}`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeepLink {
    pub record_id: String,
    pub org_id: Option<String>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeepLinkNavigation {
    pub record_id: String,
    pub org_id: String,
    pub switched_org: bool,
    /// Path the wrapped web app should open.
    pub web_path: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeepLinkError {
    #[error("not a companyos deep link")]
    NotCompanyOs,
    #[error("deep link requires org context")]
    MissingOrg,
}

impl DeepLink {
    pub fn parse(uri: &str) -> Result<Self, DeepLinkError> {
        let trimmed = uri.trim();
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with("companyos://") {
            return Err(DeepLinkError::NotCompanyOs);
        }

        let rest = &trimmed["companyos://".len()..];
        let (path_part, query_part) = match rest.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (rest, None),
        };

        let segments: Vec<&str> = path_part.split('/').filter(|s| !s.is_empty()).collect();
        let (path_org, record_id) = match segments.as_slice() {
            ["record", id] => (None, (*id).to_string()),
            ["org", org, "record", id] => (Some((*org).to_string()), (*id).to_string()),
            _ => return Err(DeepLinkError::NotCompanyOs),
        };

        let mut org_id = path_org;
        if let Some(q) = query_part {
            for pair in q.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    if k == "org" || k == "org_id" {
                        org_id = Some(v.to_string());
                    }
                }
            }
        }

        Ok(Self {
            kind: kind_from_record_id(&record_id),
            record_id,
            org_id,
        })
    }

    pub fn to_uri(&self) -> String {
        match &self.org_id {
            Some(org) => format!("companyos://record/{}?org={org}", self.record_id),
            None => format!("companyos://record/{}", self.record_id),
        }
    }
}

pub fn kind_from_record_id(record_id: &str) -> Option<String> {
    let kind = if record_id.starts_with("exp_") {
        "expense"
    } else if record_id.starts_with("dl_") {
        "deal"
    } else if record_id.starts_with("tsk_") {
        "task"
    } else if record_id.starts_with("apr_") {
        "approval"
    } else if record_id.starts_with("inv_") {
        "invoice"
    } else {
        return None;
    };
    Some(kind.into())
}

pub fn web_path_for(record_id: &str) -> String {
    match kind_from_record_id(record_id).as_deref() {
        Some("expense") => format!("/finance/expenses/{record_id}"),
        Some("deal") => format!("/sales/deals/{record_id}"),
        Some("task") => format!("/ops/tasks/{record_id}"),
        Some("approval") => format!("/approvals/{record_id}"),
        Some("invoice") => format!("/finance/invoices/{record_id}"),
        _ => format!("/record/{record_id}"),
    }
}

/// Open a deep link in the correct organization.
pub fn open_in_org(
    link: &DeepLink,
    current_org_id: Option<&str>,
) -> Result<DeepLinkNavigation, DeepLinkError> {
    let target = link
        .org_id
        .as_deref()
        .or(current_org_id)
        .ok_or(DeepLinkError::MissingOrg)?;
    let switched = current_org_id.map(|c| c != target).unwrap_or(true);
    Ok(DeepLinkNavigation {
        record_id: link.record_id.clone(),
        org_id: target.to_string(),
        switched_org: switched,
        web_path: web_path_for(&link.record_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_record_uri() {
        let link = DeepLink::parse("companyos://record/exp_01hxyz").unwrap();
        assert_eq!(link.record_id, "exp_01hxyz");
        assert_eq!(link.kind.as_deref(), Some("expense"));
        assert!(link.org_id.is_none());
    }

    #[test]
    fn parses_org_query_and_path() {
        let q = DeepLink::parse("companyos://record/dl_abc?org=org_acme").unwrap();
        assert_eq!(q.org_id.as_deref(), Some("org_acme"));
        let p = DeepLink::parse("companyos://org/org_beta/record/tsk_1").unwrap();
        assert_eq!(p.org_id.as_deref(), Some("org_beta"));
        assert_eq!(p.record_id, "tsk_1");
    }

    #[test]
    fn opens_correct_org() {
        let link = DeepLink::parse("companyos://record/apr_99?org=org_b").unwrap();
        let nav = open_in_org(&link, Some("org_a")).unwrap();
        assert!(nav.switched_org);
        assert_eq!(nav.org_id, "org_b");
        assert_eq!(nav.web_path, "/approvals/apr_99");
    }

    #[test]
    fn no_switch_when_same_org() {
        let link = DeepLink::parse("companyos://record/exp_1?org=org_acme").unwrap();
        let nav = open_in_org(&link, Some("org_acme")).unwrap();
        assert!(!nav.switched_org);
        assert_eq!(nav.web_path, "/finance/expenses/exp_1");
    }

    #[test]
    fn rejects_foreign_schemes() {
        assert_eq!(
            DeepLink::parse("https://example.com/record/x"),
            Err(DeepLinkError::NotCompanyOs)
        );
    }
}
