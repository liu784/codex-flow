use serde_json::Value;

const SENSITIVE_FRAGMENTS: &[&str] = &[
    "token",
    "cookie",
    "authorization",
    "apikey",
    "api_key",
    "secret",
    "password",
    "prompt",
    "messagebody",
    "message_body",
    "sourcecode",
    "source_code",
    "email",
];

pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    if SENSITIVE_FRAGMENTS
                        .iter()
                        .any(|fragment| normalized.contains(fragment))
                    {
                        (key.clone(), Value::String("<redacted>".into()))
                    } else {
                        (key.clone(), redact_json(value))
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_json).collect()),
        _ => value.clone(),
    }
}
