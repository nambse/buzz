use ortak_office::encrypted::{
    key_provider::{DmOfficeKeyBinding, EnvDmKeyProvider, OfficeKeyPurpose},
    publish::EncryptedDmPublisher,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    format: String,
    pair_ids: Vec<Uuid>,
    key_bindings: Vec<DmOfficeKeyBinding>,
    relay_origin: String,
}
pub(super) struct Selection {
    pub(super) pairs: Vec<Uuid>,
    pub(super) bindings: Vec<DmOfficeKeyBinding>,
    pub(super) keys: EnvDmKeyProvider,
    pub(super) publisher: EncryptedDmPublisher,
}
impl Selection {
    pub(super) fn parse(company: Uuid, value: serde_json::Value) -> Result<Self, &'static str> {
        let config: Config =
            serde_json::from_value(value).map_err(|_| "invalid encrypted selection")?;
        let pairs: BTreeSet<_> = config.pair_ids.iter().copied().collect();
        let purposes = [
            OfficeKeyPurpose::DmDecrypt,
            OfficeKeyPurpose::WrapMaster,
            OfficeKeyPurpose::UnwrapMaster,
            OfficeKeyPurpose::DmSeal,
        ];
        if config.format != "ortak-encrypted-worker/1"
            || pairs.is_empty()
            || pairs.len() > 16
            || pairs.len() != config.pair_ids.len()
            || pairs.contains(&Uuid::nil())
            || config.key_bindings.is_empty()
            || config.key_bindings.len() > 16
            || config.relay_origin.len() > 512
            || config.key_bindings.iter().any(|b| {
                b.signer.company_id != company
                    || b.purposes.len() != 4
                    || purposes.iter().any(|p| !b.purposes.contains(p))
            })
        {
            return Err("invalid encrypted selection bounds or owner");
        }
        let publisher = EncryptedDmPublisher::new(
            config
                .relay_origin
                .parse()
                .map_err(|_| "invalid encrypted relay")?,
        )
        .map_err(|_| "invalid encrypted relay")?;
        let keys = EnvDmKeyProvider::new(config.key_bindings.clone())
            .map_err(|_| "invalid encrypted key allowlist")?;
        Ok(Self {
            pairs: pairs.into_iter().collect(),
            bindings: config.key_bindings,
            keys,
            publisher,
        })
    }
}
