//! Identifiers for CompanyOS.
//!
//! Internal primary keys are UUIDv7. Public API IDs are prefixed strings
//! (`org_`, `usr_`, `inv_`, `dl_`, `cus_`, …) that encode the same UUID.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use uuid::Uuid;

/// Errors when parsing a public ID.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IdError {
    #[error("invalid public id: missing or unknown prefix")]
    InvalidPrefix,
    #[error("invalid public id: bad uuid payload: {0}")]
    InvalidUuid(String),
    #[error("invalid public id: empty")]
    Empty,
}

/// Kind of public identifier (prefix without trailing underscore).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdKind {
    Org,
    User,
    Invoice,
    CreditNote,
    Payment,
    Expense,
    Deal,
    Customer,
    Contact,
    Lead,
    Quote,
    Pipeline,
    Stage,
    Product,
    Activity,
    Import,
    Project,
    Task,
    Hello,
    Role,
    Team,
    Department,
    Invitation,
    Membership,
}

impl IdKind {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Org => "org_",
            Self::User => "usr_",
            Self::Invoice => "inv_",
            Self::CreditNote => "cn_",
            Self::Payment => "pay_",
            Self::Expense => "exp_",
            Self::Deal => "dl_",
            Self::Customer => "cus_",
            Self::Contact => "con_",
            Self::Lead => "led_",
            Self::Quote => "qte_",
            Self::Pipeline => "pl_",
            Self::Stage => "stg_",
            Self::Product => "prd_",
            Self::Activity => "act_",
            Self::Import => "imp_",
            Self::Project => "prj_",
            Self::Task => "tsk_",
            Self::Hello => "hel_",
            Self::Role => "rol_",
            Self::Team => "tem_",
            Self::Department => "dep_",
            Self::Invitation => "ivt_",
            Self::Membership => "mem_",
        }
    }

    pub fn from_prefix(s: &str) -> Option<Self> {
        match s {
            "org_" => Some(Self::Org),
            "usr_" => Some(Self::User),
            "inv_" => Some(Self::Invoice),
            "cn_" => Some(Self::CreditNote),
            "pay_" => Some(Self::Payment),
            "exp_" => Some(Self::Expense),
            "dl_" => Some(Self::Deal),
            "cus_" => Some(Self::Customer),
            "con_" => Some(Self::Contact),
            "led_" => Some(Self::Lead),
            "qte_" => Some(Self::Quote),
            "pl_" => Some(Self::Pipeline),
            "stg_" => Some(Self::Stage),
            "prd_" => Some(Self::Product),
            "act_" => Some(Self::Activity),
            "imp_" => Some(Self::Import),
            "prj_" => Some(Self::Project),
            "tsk_" => Some(Self::Task),
            "hel_" => Some(Self::Hello),
            "rol_" => Some(Self::Role),
            "tem_" => Some(Self::Team),
            "dep_" => Some(Self::Department),
            "ivt_" => Some(Self::Invitation),
            "mem_" => Some(Self::Membership),
            _ => None,
        }
    }
}

/// Generate a new UUIDv7 internal primary key.
pub fn new_uuid_v7() -> Uuid {
    Uuid::now_v7()
}

/// A typed public ID: `{prefix}{uuid}` (uuid is hyphenated lowercase).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PublicId {
    kind: IdKind,
    uuid: Uuid,
}

impl PublicId {
    pub fn new(kind: IdKind, uuid: Uuid) -> Self {
        Self { kind, uuid }
    }

    pub fn generate(kind: IdKind) -> Self {
        Self {
            kind,
            uuid: new_uuid_v7(),
        }
    }

    pub fn kind(&self) -> IdKind {
        self.kind
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn as_str(&self) -> String {
        format!("{}{}", self.kind.prefix(), self.uuid)
    }
}

impl fmt::Display for PublicId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.kind.prefix(), self.uuid)
    }
}

impl FromStr for PublicId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(IdError::Empty);
        }
        // Known prefixes (longest first where needed).
        const PREFIXES: &[(&str, IdKind)] = &[
            ("org_", IdKind::Org),
            ("usr_", IdKind::User),
            ("inv_", IdKind::Invoice),
            ("pay_", IdKind::Payment),
            ("exp_", IdKind::Expense),
            ("cus_", IdKind::Customer),
            ("con_", IdKind::Contact),
            ("led_", IdKind::Lead),
            ("qte_", IdKind::Quote),
            ("prd_", IdKind::Product),
            ("act_", IdKind::Activity),
            ("imp_", IdKind::Import),
            ("prj_", IdKind::Project),
            ("tsk_", IdKind::Task),
            ("stg_", IdKind::Stage),
            ("hel_", IdKind::Hello),
            ("rol_", IdKind::Role),
            ("tem_", IdKind::Team),
            ("dep_", IdKind::Department),
            ("ivt_", IdKind::Invitation),
            ("mem_", IdKind::Membership),
            ("pl_", IdKind::Pipeline),
            ("dl_", IdKind::Deal),
            ("cn_", IdKind::CreditNote),
        ];
        for (prefix, kind) in PREFIXES {
            if let Some(rest) = s.strip_prefix(prefix) {
                let uuid =
                    Uuid::parse_str(rest).map_err(|e| IdError::InvalidUuid(e.to_string()))?;
                return Ok(Self { kind: *kind, uuid });
            }
        }
        Err(IdError::InvalidPrefix)
    }
}

impl Serialize for PublicId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for PublicId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Convenience constructors.
pub fn org_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Org, uuid)
}

pub fn usr_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::User, uuid)
}

pub fn inv_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Invoice, uuid)
}

pub fn cn_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::CreditNote, uuid)
}

pub fn pay_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Payment, uuid)
}

pub fn exp_id(uuid: Uuid) -> PublicId {
    PublicId::new(IdKind::Expense, uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v7_is_version_7() {
        let id = new_uuid_v7();
        assert_eq!(id.get_version_num(), 7);
    }

    #[test]
    fn round_trip_org() {
        let uuid = new_uuid_v7();
        let pub_id = org_id(uuid);
        assert!(pub_id.as_str().starts_with("org_"));
        let parsed: PublicId = pub_id.as_str().parse().unwrap();
        assert_eq!(parsed.kind(), IdKind::Org);
        assert_eq!(parsed.uuid(), uuid);
    }

    #[test]
    fn round_trip_usr_inv() {
        let u = new_uuid_v7();
        let usr = usr_id(u);
        assert_eq!(usr.to_string().parse::<PublicId>().unwrap(), usr);

        let inv = inv_id(u);
        assert!(inv.as_str().starts_with("inv_"));
        assert_eq!(inv.to_string().parse::<PublicId>().unwrap(), inv);
    }

    #[test]
    fn generate_and_parse_all_kinds() {
        for kind in [
            IdKind::Org,
            IdKind::User,
            IdKind::Invoice,
            IdKind::CreditNote,
            IdKind::Payment,
            IdKind::Expense,
            IdKind::Deal,
            IdKind::Customer,
            IdKind::Project,
            IdKind::Task,
            IdKind::Hello,
            IdKind::Role,
            IdKind::Team,
            IdKind::Department,
            IdKind::Invitation,
            IdKind::Membership,
        ] {
            let id = PublicId::generate(kind);
            let parsed: PublicId = id.to_string().parse().unwrap();
            assert_eq!(parsed, id);
        }
    }

    #[test]
    fn serde_round_trip() {
        let id = PublicId::generate(IdKind::Org);
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("org_"));
        let back: PublicId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn rejects_bad_prefix_and_empty() {
        assert_eq!("".parse::<PublicId>().unwrap_err(), IdError::Empty);
        assert!(matches!(
            "foo_0190".parse::<PublicId>().unwrap_err(),
            IdError::InvalidPrefix
        ));
        assert!(matches!(
            "org_not-a-uuid".parse::<PublicId>().unwrap_err(),
            IdError::InvalidUuid(_)
        ));
    }

    #[test]
    fn display_matches_as_str() {
        let id = PublicId::generate(IdKind::Deal);
        assert_eq!(id.to_string(), id.as_str());
        assert!(id.as_str().starts_with("dl_"));
    }
}
