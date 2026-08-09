//! Service-neutral request and response models for registry account features.

use sea_orm::prelude::{Json, Uuid};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    /// `shared_auth.principals.shared_user_id` on the realm's auth instance.
    pub subject: Uuid,
    /// Which auth instance that principal lives on: `customer` or `admin`.
    pub realm: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserSummary {
    pub id: Uuid,
    pub subject: Uuid,
    pub realm: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub settings: Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrgSummary {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub org_id: Uuid,
    pub org_slug: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackageSummary {
    pub id: Uuid,
    pub org_id: Uuid,
    pub org_slug: String,
    pub project_id: Option<Uuid>,
    pub project_slug: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub repo_url: String,
    pub config: Json,
    pub latest_version: Option<String>,
    pub download_count: i64,
    pub version_count: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HomePageData {
    pub user: Option<UserSummary>,
    pub orgs: Vec<OrgSummary>,
    pub projects: Vec<ProjectSummary>,
    pub packages: Vec<PackageSummary>,
    pub query: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrgDashboardData {
    pub org: OrgSummary,
    pub projects: Vec<ProjectSummary>,
    pub packages: Vec<PackageSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvitationReceipt {
    pub invitation_id: Uuid,
    /// One-time invitation token. Only the caller receives this value; the
    /// database stores its SHA-256 digest.
    pub token: String,
    pub email: String,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PackageSettingsInput {
    pub description: Option<String>,
    pub project_id: Option<Uuid>,
    pub visibility: String,
    pub config: Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserSettingsInput {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub settings: Json,
}
