//! Versioned protected Gmail grant codec; no browser, network or raw diagnostics.
use crate::config_validate::{get, object, text};
use zeroize::Zeroizing;
pub(super) const SEND: &str = "https://www.googleapis.com/auth/gmail.send";
pub(super) struct Grant {
    pub(super) client_id: Zeroizing<String>,
    pub(super) client_secret: Zeroizing<String>,
    pub(super) subject: Zeroizing<String>,
    pub(super) email: Zeroizing<String>,
    pub(super) refresh: Zeroizing<String>,
    pub(super) access: Zeroizing<String>,
    pub(super) expires: u64,
}
fn token(s: &str, max: usize) -> Result<Zeroizing<String>, ()> {
    if s.is_empty() || s.len() > max || !s.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(());
    }
    Ok(Zeroizing::new(s.into()))
}
impl Grant {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() > 16384 {
            return Err(());
        }
        let value = crate::config_decode::decode(bytes).map_err(|_| ())?;
        let fields = object(&value).map_err(|_| ())?;
        let keys = [
            "schema_version",
            "client_id",
            "client_secret",
            "subject",
            "email",
            "refresh_token",
            "access_token",
            "expires_at",
            "scopes",
        ];
        if fields.len() != keys.len()
            || keys.iter().any(|key| get(fields, key).is_err())
            || !matches!(
                get(fields, "schema_version"),
                Ok(crate::config_decode::Value::Integer(1))
            )
        {
            return Err(());
        }
        let field = |key, max| {
            token(
                text(get(fields, key).map_err(|_| ())?).map_err(|_| ())?,
                max,
            )
        };
        let email = field("email", 254)?;
        crate::smtp_address::address(&email)?;
        let scopes = crate::config_validate::list(get(fields, "scopes").map_err(|_| ())?, 3, 3)
            .map_err(|_| ())?;
        let scopes = scopes
            .iter()
            .map(|v| text(v).map_err(|_| ()))
            .collect::<Result<Vec<_>, _>>()?;
        if scopes.len() != 3 || ![SEND, "openid", "email"].iter().all(|s| scopes.contains(s)) {
            return Err(());
        }
        let expires = match get(fields, "expires_at") {
            Ok(crate::config_decode::Value::Integer(n)) if *n <= 4_102_444_799 => *n,
            _ => return Err(()),
        };
        Ok(Self {
            client_id: field("client_id", 256)?,
            client_secret: field("client_secret", 4096)?,
            subject: field("subject", 128)?,
            email,
            refresh: field("refresh_token", 4096)?,
            access: field("access_token", 4096)?,
            expires,
        })
    }
    pub(super) fn encode(&self) -> Result<Zeroizing<Vec<u8>>, ()> {
        use serde_core::{ser::SerializeMap, Serializer};
        let mut bytes = Zeroizing::new(Vec::new());
        let mut serializer = serde_json::Serializer::new(&mut *bytes);
        let mut map = serializer.serialize_map(Some(9)).map_err(|_| ())?;
        map.serialize_entry("schema_version", &1).map_err(|_| ())?;
        for (key, value) in [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("subject", self.subject.as_str()),
            ("email", self.email.as_str()),
            ("refresh_token", self.refresh.as_str()),
            ("access_token", self.access.as_str()),
        ] {
            map.serialize_entry(key, value).map_err(|_| ())?;
        }
        map.serialize_entry("expires_at", &self.expires)
            .map_err(|_| ())?;
        map.serialize_entry("scopes", &[SEND, "openid", "email"])
            .map_err(|_| ())?;
        map.end().map_err(|_| ())?;
        Self::parse(&bytes)?;
        Ok(bytes)
    }
    pub(super) fn matches(&self, profile: &crate::email_profile::Profile) -> bool {
        self.subject.as_str() == profile.account.as_str()
            && self.email.as_str() == profile.sender.as_str()
            && !profile.client_id.is_empty()
            && self.client_id.as_str() == profile.client_id.as_str()
    }
    pub(super) fn usable(&self, now: u64, deadline: u64) -> bool {
        self.expires > now.saturating_add(deadline).saturating_add(30)
    }
    pub(super) fn update(&mut self, bytes: &[u8], now: u64) -> Result<(), ()> {
        if bytes.len() > 8192 {
            return Err(());
        }
        let value = crate::config_decode::decode(bytes).map_err(|_| ())?;
        let fields = object(&value).map_err(|_| ())?;
        if text(get(fields, "token_type").map_err(|_| ())?).map_err(|_| ())? != "Bearer" {
            return Err(());
        }
        let access = token(
            text(get(fields, "access_token").map_err(|_| ())?).map_err(|_| ())?,
            4096,
        )?;
        let lifetime = match get(fields, "expires_in") {
            Ok(crate::config_decode::Value::Integer(n)) if (60..=86400).contains(n) => *n,
            _ => return Err(()),
        };
        let expires = now
            .checked_add(lifetime)
            .filter(|t| *t <= 4_102_444_799)
            .ok_or(())?;
        let refresh = if let Ok(v) = get(fields, "refresh_token") {
            Some(token(text(v).map_err(|_| ())?, 4096)?)
        } else {
            None
        };
        if let Ok(v) = get(fields, "scope") {
            let scope = text(v).map_err(|_| ())?;
            let scopes = scope.split_ascii_whitespace().collect::<Vec<_>>();
            // Google may return canonical userinfo.email for the requested email scope.
            if scopes.len() != 3
                || !scopes.contains(&SEND)
                || !scopes.contains(&"openid")
                || !(scopes.contains(&"email")
                    || scopes.contains(&"https://www.googleapis.com/auth/userinfo.email"))
            {
                return Err(());
            }
        }
        self.access = access;
        self.expires = expires;
        if let Some(refresh) = refresh {
            self.refresh = refresh;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn strict_grants_rotation_scope_and_failure_are_transactional() {
        let fixture = serde_json::json!({"schema_version":1,"client_id":"desktop-fixture","client_secret":"fake-secret","subject":"fixture-account","email":"s@fixture.invalid","refresh_token":"old-refresh","access_token":"old-access","expires_at":0,"scopes":[super::SEND,"openid","email"]});
        let mut grant = super::Grant::parse(&serde_json::to_vec(&fixture).unwrap()).unwrap();
        assert!(!grant.usable(0, 10));
        let initial = grant.encode().unwrap();
        for invalid in [
            serde_json::json!({"token_type":"Bearer","access_token":"new","expires_in":3600,"scope":"openid email"}),
            serde_json::json!({"token_type":"Other","access_token":"new","expires_in":3600}),
            serde_json::json!({"token_type":"Bearer","access_token":"new","expires_in":0}),
        ] {
            assert!(grant
                .update(&serde_json::to_vec(&invalid).unwrap(), 100)
                .is_err());
            assert_eq!(initial.as_slice(), grant.encode().unwrap().as_slice());
        }
        grant
            .update(
                br#"{"token_type":"Bearer","access_token":"new","expires_in":3600}"#,
                100,
            )
            .unwrap();
        assert_eq!(grant.refresh.as_str(), "old-refresh");
        assert!(grant.usable(100, 30));
        grant.update(br#"{"token_type":"Bearer","access_token":"next","refresh_token":"rotated","expires_in":3600,"scope":"openid email https://www.googleapis.com/auth/gmail.send"}"#,200).unwrap();
        assert_eq!(grant.refresh.as_str(), "rotated");
        assert_eq!(
            super::Grant::parse(&grant.encode().unwrap())
                .unwrap()
                .expires,
            3800
        );
        let mut extra = fixture.clone();
        extra["unexpected"] = serde_json::json!(true);
        assert!(super::Grant::parse(&serde_json::to_vec(&extra).unwrap()).is_err());
        let mut broad = fixture;
        broad["scopes"] = serde_json::json!([super::SEND, "openid", "gmail.readonly"]);
        assert!(super::Grant::parse(&serde_json::to_vec(&broad).unwrap()).is_err());
    }
}
