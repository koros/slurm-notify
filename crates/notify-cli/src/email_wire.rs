//! Deterministic credential-free provider requests. Exact bytes are suitable for freezing.
use crate::{
    email_content::Content,
    email_profile::{Profile, Provider},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use zeroize::Zeroizing;
pub(super) struct Request {
    pub(super) content_type: String,
    pub(super) body: Zeroizing<Vec<u8>>,
}
pub(super) fn prepare(profile: &Profile, content: &Content) -> Result<Request, ()> {
    let (content_type,body)=match profile.provider {
        Provider::SendGrid => ("application/json".into(),serde_json::to_vec(&serde_json::json!({
            "personalizations":[{"to":[{"email":profile.recipient.as_str()}]}],
            "from":{"email":profile.sender.as_str()},"subject":content.subject,
            "content":[{"type":"text/plain","value":content.text.as_str()}],
            "headers":{"Message-ID":format!("<{}@{}>",content.id,crate::smtp_message::ID_DOMAIN)},
            "tracking_settings":{"click_tracking":{"enable":false,"enable_text":false},"open_tracking":{"enable":false},"subscription_tracking":{"enable":false},"ganalytics":{"enable":false}}
        })).map_err(|_|())?),
        Provider::Mailgun=> {
            let boundary=format!("sn_email_v1_{}",content.id);
            let marker=format!("--{boundary}");
            let id=format!("<{}@{}>",content.id,crate::smtp_message::ID_DOMAIN);
            let mut bytes=Vec::new();
            for (name,value) in [("from",profile.sender.as_str()),("to",profile.recipient.as_str()),("subject",content.subject.as_str()),("text",content.text.as_str()),("h:Message-Id",id.as_str()),("o:tracking","no"),("o:tracking-clicks","no"),("o:tracking-opens","no")] {
                if value.split('\n').any(|line|line.trim_end_matches('\r').starts_with(&marker)) || value.contains('\r') {return Err(());}
                bytes.extend_from_slice(format!("{marker}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
            }
            bytes.extend_from_slice(format!("{marker}--\r\n").as_bytes());
            (format!("multipart/form-data; boundary={boundary}"),bytes)
        },
        Provider::Gmail=> {
            let mime=content.mime(&profile.sender,&profile.recipient)?;
            ("application/json".into(),serde_json::to_vec(&serde_json::json!({"raw":URL_SAFE_NO_PAD.encode(mime.as_slice())})).map_err(|_|())?)
        }
    };
    if body.len() > profile.payload_limit {
        return Err(());
    }
    Ok(Request {
        content_type,
        body: Zeroizing::new(body),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_provider_requests_and_expansion_bounds() {
        let id = crate::public_event::identifier([0; 16]);
        let mut content = Content::synthetic(&id, 0).unwrap();
        for kind in ["sendgrid", "mailgun", "gmail_api"] {
            let v = crate::email_profile::tests::fixture(kind);
            let mut p = Profile::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
            let a = prepare(&p, &content).unwrap();
            let b = prepare(&p, &content).unwrap();
            assert_eq!(a.body, b.body);
            assert_eq!(a.content_type, b.content_type);
            match p.provider {
                Provider::SendGrid => {
                    let v: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
                    assert_eq!(v["tracking_settings"]["open_tracking"]["enable"], false);
                    assert_eq!(
                        v["personalizations"][0]["to"][0]["email"],
                        "r@fixture.invalid"
                    );
                }
                Provider::Mailgun => assert!(
                    String::from_utf8_lossy(&a.body).contains("name=\"o:tracking\"\r\n\r\nno")
                ),
                Provider::Gmail => {
                    let v: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
                    let mime = URL_SAFE_NO_PAD.decode(v["raw"].as_str().unwrap()).unwrap();
                    assert!(mime.starts_with(b"From:"));
                    assert!(!mime.ends_with(b"\r\n.\r\n"));
                }
            }
            p.payload_limit = a.body.len() - 1;
            assert!(prepare(&p, &content).is_err());
        }
        let p = Profile::parse(
            &serde_json::to_vec(&crate::email_profile::tests::fixture("mailgun")).unwrap(),
        )
        .unwrap();
        content.text = Zeroizing::new(format!("--sn_email_v1_{id}\n"));
        assert!(prepare(&p, &content).is_err());
    }
}
