//! Provider factories the desktop client registers with the engine.

use std::sync::Arc;

use arcvox_core::models::ModelStore;
use arcvox_core::ProviderFactory;
use arcvox_inference::factory::EmbeddedFactory;

pub const BUNDLED_MANIFEST: &str = include_str!("../resources/models-manifest.json");

pub fn build_factories(store: Arc<ModelStore>) -> Vec<Arc<dyn ProviderFactory>> {
    vec![Arc::new(EmbeddedFactory::new(store, true))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::config::ProviderKind;
    use arcvox_core::DataDir;

    #[test]
    fn factories_cover_embedded_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        dirs.ensure().unwrap();
        let store = Arc::new(ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            BUNDLED_MANIFEST,
            None,
        ));
        let kinds: Vec<ProviderKind> = build_factories(store).iter().map(|f| f.kind()).collect();
        assert!(kinds.contains(&ProviderKind::Embedded));
    }
}
