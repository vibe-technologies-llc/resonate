use crate::{
    Action, Asked, Error, Result,
    description::{Service, document},
    http::{self, Asking},
};

pub(crate) struct Answered(Vec<(String, String)>);

impl Answered {
    pub fn named(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, value)| value.as_str())
    }
}

pub(crate) fn call(
    service: &Service,
    action: Action,
    arguments: &[(&str, &str)],
) -> Result<Answered> {
    let asked = Asked::Action(action);
    let envelope = envelope(&service.kind, action, arguments);
    let soap_action = format!("\"{}#{}\"", service.kind, action.name());
    let answer = http::exchange(
        &Asking {
            method: "POST",
            to: &service.control,
            headers: &[
                ("Content-Type", "text/xml; charset=\"utf-8\""),
                ("SOAPAction", &soap_action),
            ],
            body: envelope.as_bytes(),
        },
        asked,
    )?;

    let text = std::str::from_utf8(&answer.body).map_err(|_| Error::NotText { asked });
    match answer.status {
        200 => answered(text?, action),
        500 => Err(Error::Fault {
            action,
            code: text.ok().and_then(|text| fault_code(text, action)),
        }),
        status => Err(Error::Status { asked, status }),
    }
}

pub(crate) fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for letter in text.chars() {
        match letter {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            letter => escaped.push(letter),
        }
    }
    escaped
}

fn envelope(kind: &str, action: Action, arguments: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (name, value) in arguments {
        body.push_str(&format!("<{name}>{}</{name}>", escaped(value)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<s:Envelope \
         xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{name} \
         xmlns:u=\"{}\">{body}</u:{name}></s:Body></s:Envelope>",
        escaped(kind),
        name = action.name(),
    )
}

fn answered(text: &str, action: Action) -> Result<Answered> {
    let document = document(text, Asked::Action(action))?;
    let response = format!("{}Response", action.name());
    let answer = document
        .descendants()
        .find(|node| node.tag_name().name() == response)
        .ok_or(Error::Unanswered { action })?;
    Ok(Answered(
        answer
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|argument| {
                (
                    argument.tag_name().name().to_owned(),
                    argument.text().unwrap_or_default().to_owned(),
                )
            })
            .collect(),
    ))
}

fn fault_code(text: &str, action: Action) -> Option<u32> {
    let document = document(text, Asked::Action(action)).ok()?;
    document
        .descendants()
        .find(|node| node.tag_name().name() == "errorCode")
        .and_then(|node| node.text())
        .and_then(|code| code.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_names_the_action_in_its_service_and_escapes_what_it_carries() {
        let envelope = envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            Action::SetAvTransportUri,
            &[("InstanceID", "0"), ("CurrentURI", "http://h/a?b=1&c=<2>")],
        );

        assert!(envelope.contains(
            "<u:SetAVTransportURI xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">"
        ));
        assert!(envelope.contains("<CurrentURI>http://h/a?b=1&amp;c=&lt;2&gt;</CurrentURI>"));
        assert!(envelope.ends_with("</u:SetAVTransportURI></s:Body></s:Envelope>"));
    }

    #[test]
    fn an_answer_is_read_by_its_arguments_and_a_fault_by_its_code() {
        let answer = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:GetPositionInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><Track>1</Track><RelTime>0:01:02.500</RelTime></u:GetPositionInfoResponse></s:Body></s:Envelope>"#;
        let fault = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>701</errorCode></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#;

        let read = answered(answer, Action::GetPositionInfo).expect("reads");

        assert_eq!(read.named("RelTime"), Some("0:01:02.500"));
        assert_eq!(read.named("Missing"), None);
        assert_eq!(fault_code(fault, Action::Play), Some(701));
        assert!(matches!(
            answered(answer, Action::Play),
            Err(Error::Unanswered {
                action: Action::Play
            })
        ));
    }
}
