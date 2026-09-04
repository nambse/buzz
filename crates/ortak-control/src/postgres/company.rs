use ortak_domain::RoutingPolicy;
use sqlx::Row;
use uuid::Uuid;

use super::PgControlPlane;
use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::ports::CompanyDirectory;

impl CompanyDirectory for PgControlPlane {
    async fn resolve_company_for_community(&self, community_id: Uuid) -> Result<CompanyScope> {
        let row =
            sqlx::query("SELECT company_id FROM office_company_bindings WHERE community_id = $1")
                .bind(community_id)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(row) => Ok(CompanyScope::new(
                row.try_get("company_id")?,
                Some(community_id),
            )),
            None => Err(ControlError::UnknownCompanyBinding { community_id }),
        }
    }

    async fn resolve_company_by_slug(&self, slug: &str) -> Result<CompanyScope> {
        let row = sqlx::query(
            "SELECT c.id, b.community_id
               FROM companies c
               LEFT JOIN office_company_bindings b ON b.company_id = c.id
              WHERE c.slug = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok(CompanyScope::new(
                row.try_get("id")?,
                row.try_get("community_id")?,
            )),
            None => Err(ControlError::UnknownCompany {
                slug: slug.to_owned(),
            }),
        }
    }

    async fn routing_policy(&self, scope: &CompanyScope) -> Result<RoutingPolicy> {
        let row = sqlx::query("SELECT routing_policy FROM companies WHERE id = $1")
            .bind(scope.company_id())
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => parse_policy(row.try_get("routing_policy")?),
            None => Err(ControlError::InvalidData(format!(
                "company {} has no registry row",
                scope.company_id()
            ))),
        }
    }
}

/// Parses the company policy column. The schema default `{}` means the
/// domain default policy; any other value must be a complete, valid policy.
pub(crate) fn parse_policy(value: serde_json::Value) -> Result<RoutingPolicy> {
    let policy = match &value {
        serde_json::Value::Object(fields) if fields.is_empty() => RoutingPolicy::default(),
        _ => serde_json::from_value(value)?,
    };
    policy.validate()?;
    Ok(policy)
}
