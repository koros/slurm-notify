//! Explicit existing-only recovery; no environment, scheduler or network access.
use crate::config_validate::{get, object, text};
pub(super) fn resume(path: &std::path::Path, alias: &str) -> Result<bool, ()> {
    let snapshot = crate::config_snapshot::read(path).map_err(|_| ())?;
    let draft = crate::config_validate::validate(snapshot.bytes()).map_err(|_| ())?;
    let root = object(&draft.value).map_err(|_| ())?;
    let monitor = object(get(root, "monitor").map_err(|_| ())?).map_err(|_| ())?;
    let cluster = text(get(monitor, "cluster_id").map_err(|_| ())?).map_err(|_| ())?;
    let providers = object(get(root, "providers").map_err(|_| ())?).map_err(|_| ())?;
    let provider = get(providers, alias).map_err(|_| ())?;
    let resources = crate::email_resources::Resources::new(
        provider,
        get(root, "delivery").map_err(|_| ())?,
        cluster,
        alias,
    )?;
    if !resources.approved() {
        return Err(());
    }
    let storage = object(get(root, "storage").map_err(|_| ())?).map_err(|_| ())?;
    let raw = text(get(storage, "path").map_err(|_| ())?).map_err(|_| ())?;
    let parent = crate::state_path::database_parent(raw).map_err(|_| ())?;
    let timeout = match get(monitor, "query_timeout_seconds").map_err(|_| ())? {
        crate::config_decode::Value::Integer(n) => std::time::Duration::from_secs(*n),
        _ => return Err(()),
    };
    let mut store =
        crate::storage::Store::open_existing_private(parent, timeout).map_err(|_| ())?;
    if store
        .email_pause(cluster, alias, resources.revision())
        .map_err(|_| ())?
        .is_none()
    {
        return Ok(false);
    }
    store
        .resume_email(cluster, alias, resources.revision())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    #[test]
    fn recovery_is_existing_only_exact_and_excludes_active_owner() {
        use std::{os::unix::fs::PermissionsExt, time::Duration};
        let d = crate::storage::tests::Directory::new();
        let base = d.path().parent().unwrap().canonicalize().unwrap();
        let state = base.join("recovery-state");
        let config = base.join("api-config.json");
        let mut v: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/config/sendgrid-v1.example.json"
        ))
        .unwrap();
        v["monitor"]["cluster_id"] = serde_json::json!("cluster");
        v["storage"]["path"] = serde_json::json!(state.join("state.sqlite3"));
        let draft = crate::config_validate::validate(&serde_json::to_vec(&v).unwrap()).unwrap();
        let root = crate::config_validate::object(&draft.value).unwrap();
        let providers =
            crate::config_validate::object(crate::config_validate::get(root, "providers").unwrap())
                .unwrap();
        let r = crate::email_resources::Resources::new(
            crate::config_validate::get(providers, "primary").unwrap(),
            crate::config_validate::get(root, "delivery").unwrap(),
            "cluster",
            "primary",
        )
        .unwrap();
        let (rendering, provider) = r.revisions();
        v["delivery"]["api_email_permissions"] = serde_json::json!([{"cluster_id":"cluster","destination":"primary","rendering_revision":rendering,"provider_revision":provider}]);
        let write = |v: &serde_json::Value| {
            std::fs::write(&config, serde_json::to_vec(v).unwrap()).unwrap();
            std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
        };
        write(&v);
        assert!(super::resume(&config, "primary").is_err());
        assert!(!state.exists());
        let mut store =
            crate::storage::Store::open_private(&state, Duration::from_millis(50)).unwrap();
        let mut candidate = crate::lifecycle::Reconciler::with_capacity(10);
        let events = candidate
            .observe_snapshot(
                "",
                "cluster|123|alice|s|a|COMPLETED|0:0",
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        store
            .persist_public_webhooks(&candidate, &events, ("cluster", "window", 0), |_| {
                vec![crate::storage::PublicRoute {
                    destination: "primary",
                    revision: r.revision(),
                    slack: None,
                    email: Some(&r.profile),
                }]
            })
            .unwrap();
        store
            .pause_email_before_attempt(
                "cluster",
                "primary",
                r.revision(),
                crate::storage::email::PauseReason::Credentials,
                0,
            )
            .unwrap();
        assert!(super::resume(&config, "primary").is_err());
        drop(store);
        v["providers"]["primary"]["recipient"] = serde_json::json!("changed@fixture.invalid");
        write(&v);
        assert!(super::resume(&config, "primary").is_err());
        v["providers"]["primary"]["recipient"] = serde_json::json!(r.profile.recipient.as_str());
        write(&v);
        assert!(super::resume(&config, "primary").unwrap());
        assert!(!super::resume(&config, "primary").unwrap());
        assert!(super::resume(&config, "unknown").is_err());
        let store = crate::storage::Store::open_existing_private(&state, Duration::from_millis(50))
            .unwrap();
        assert_eq!(
            store.due_deliveries("cluster", 0, 10).unwrap()[0].attempt_number,
            1
        );
    }
}
