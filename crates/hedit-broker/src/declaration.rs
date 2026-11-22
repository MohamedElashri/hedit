//! Reviewed manifest vocabulary. Declarations request capabilities; only the
//! host can select resources, approve profiles, or create execution authority.
use crate::{network::Endpoint, workspace::Right};
use hedit_plugin::options::{self, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum Capability {
    #[serde(rename = "document.read")]
    DocumentRead,
    #[serde(rename = "document.edit")]
    DocumentEdit,
    #[serde(rename = "events.observe")]
    Observe,
    #[serde(rename = "ui.present")]
    Present,
    #[serde(rename = "workspace.discover")]
    Discover,
    #[serde(rename = "workspace.read")]
    Read,
    #[serde(rename = "workspace.edit")]
    Edit,
    #[serde(rename = "workspace.watch")]
    Watch,
    #[serde(rename = "tools.run")]
    Tool,
    #[serde(rename = "network.https")]
    Https,
}
impl Capability {
    pub fn name(self) -> &'static str {
        match self {
            Self::DocumentRead => "document.read",
            Self::DocumentEdit => "document.edit",
            Self::Observe => "events.observe",
            Self::Present => "ui.present",
            Self::Discover => "workspace.discover",
            Self::Read => "workspace.read",
            Self::Edit => "workspace.edit",
            Self::Watch => "workspace.watch",
            Self::Tool => "tools.run",
            Self::Https => "network.https",
        }
    }
    pub fn workspace(self) -> Option<Right> {
        match self {
            Self::Discover => Some(Right::Discover),
            Self::Read => Some(Right::Read),
            Self::Edit => Some(Right::Edit),
            Self::Watch => Some(Right::Watch),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Method {
    GET,
    POST,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDefinition {
    pub url: String,
    pub method: Method,
    pub upload: usize,
    pub response: usize,
    #[serde(default)]
    pub content_type: crate::network::ContentType,
}
impl EndpointDefinition {
    pub fn endpoint(&self) -> Result<Endpoint, String> {
        Endpoint::reviewed_content(&self.url,self.method==Method::POST,self.upload,self.response,self.content_type).map_err(|_|"Declare a public HTTPS endpoint on port 443 with explicit bounded upload and response sizes.".into())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProtocol {
    Filter,
    LanguageServer,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Argument {
    Literal(String),
    Choice(String),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub protocol: ToolProtocol,
    pub workspace: bool,
    #[serde(deserialize_with = "arguments")]
    pub arguments: Vec<Argument>,
    #[serde(default, deserialize_with = "unique_map")]
    pub choices: BTreeMap<String, Choices>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Choices(#[serde(deserialize_with = "choice_values")] pub Vec<Value>);
impl std::ops::Deref for Choices {
    type Target = [Value];
    fn deref(&self) -> &[Value] {
        &self.0
    }
}
fn choice_values<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Value>, D::Error> {
    vector::<_, _, 16>(d)
}
impl ToolDefinition {
    pub fn validate(&self) -> Result<(), String> {
        let invalid = || {
            "Declare at most 32 literal/finite-choice tool arguments, 16 choice sets, and 4 KiB of argument text.".to_string()
        };
        if self.arguments.len() > 32 || self.choices.len() > 16 {
            return Err(invalid());
        }
        let mut used = BTreeSet::new();
        let mut bytes = 0;
        let mut maximum_argv = 0;
        for argument in &self.arguments {
            match argument {
                Argument::Literal(value) => {
                    if value.len() > 256 || value.chars().any(char::is_control) {
                        return Err(invalid());
                    }
                    bytes += value.len();
                    maximum_argv += value.len() + 1;
                }
                Argument::Choice(key) => {
                    if !options::valid_id(key) || !self.choices.contains_key(key) {
                        return Err(invalid());
                    }
                    used.insert(key);
                    maximum_argv += self.choices[key]
                        .iter()
                        .map(|value| argument_text(value).len() + 1)
                        .max()
                        .unwrap_or(0);
                }
            }
        }
        for (key, values) in &self.choices {
            if !options::valid_id(key)
                || !used.contains(key)
                || values.is_empty()
                || values.len() > 16
                || values.iter().any(|value| !value.valid())
            {
                return Err(invalid());
            }
            let mut unique = BTreeSet::new();
            for value in values.iter() {
                let text = serde_json::to_string(value).map_err(|_| invalid())?;
                if !unique.insert(text.clone()) {
                    return Err(invalid());
                }
                bytes += text.len();
            }
        }
        if bytes > 4096 || maximum_argv > 4096 {
            return Err(invalid());
        }
        Ok(())
    }
    /// Resolve one reviewed argv schema. No shell, environment, executable path,
    /// or extra switches can be supplied by a callback.
    pub fn argv(&self, values: &hedit_plugin::sdk::Storage) -> Result<Vec<String>, String> {
        self.validate()?;
        values
            .validate()
            .map_err(|_| "Invalid tool argument choices.")?;
        if values.0.len() != self.choices.len() {
            return Err("Supply exactly the declared tool choices.".into());
        }
        for (key, value) in &values.0 {
            if !self
                .choices
                .get(key)
                .is_some_and(|choices| choices.iter().any(|choice| same(choice, value)))
            {
                return Err("A tool argument is outside its reviewed choices.".into());
            }
        }
        self.arguments
            .iter()
            .map(|argument| match argument {
                Argument::Literal(value) => Ok(value.clone()),
                Argument::Choice(key) => Ok(argument_text(
                    values.0.get(key).ok_or("Missing tool argument choice.")?,
                )),
            })
            .collect()
    }
}
fn argument_text(value: &Value) -> String {
    match value {
        Value::Boolean(value) => value.to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Text(value) => value.clone(),
    }
}
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Integer(a), Value::Integer(b)) => a == b,
        (Value::Text(a), Value::Text(b)) => a == b,
        _ => false,
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(deserialize_with = "capabilities")]
    pub capabilities: Vec<Capability>,
    #[serde(default, deserialize_with = "unique_map")]
    pub tools: BTreeMap<String, ToolDefinition>,
    #[serde(default, deserialize_with = "unique_map")]
    pub endpoints: BTreeMap<String, EndpointDefinition>,
}
impl Definition {
    pub fn has(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
    pub fn canonical(&self) -> Vec<Capability> {
        let mut values = self.capabilities.clone();
        values.sort();
        values
    }
    pub fn validate(&self, provider: bool) -> Result<(), String> {
        validate_capabilities(&self.capabilities, provider)?;
        if self.tools.len() > 8
            || self.endpoints.len() > 8
            || self.has(Capability::Tool) != !self.tools.is_empty()
            || self.has(Capability::Https) != !self.endpoints.is_empty()
        {
            return Err(
                "Declare 1–8 reviewed profiles/endpoints for each requested tool/HTTPS capability."
                    .into(),
            );
        }
        for (id, tool) in &self.tools {
            if !options::valid_id(id) {
                return Err("Invalid tool profile ID.".into());
            }
            tool.validate()?;
            if tool.protocol == ToolProtocol::LanguageServer && !self.has(Capability::DocumentRead)
            {
                return Err("A language-server profile requires document.read.".into());
            }
            if tool.workspace && (!self.has(Capability::Read) || !self.has(Capability::Discover)) {
                return Err(
                    "A project tool snapshot requires workspace.discover and workspace.read."
                        .into(),
                );
            }
        }
        for (id, endpoint) in &self.endpoints {
            if !options::valid_id(id) {
                return Err("Invalid endpoint ID.".into());
            }
            endpoint.endpoint()?;
        }
        Ok(())
    }
}
/// Shared structural validation for exact-source approval records and manifests.
pub fn validate_capabilities(capabilities: &[Capability], provider: bool) -> Result<(), String> {
    let has = |capability| capabilities.contains(&capability);
    if capabilities.is_empty()
        || capabilities.len() > 10
        || capabilities.iter().collect::<BTreeSet<_>>().len() != capabilities.len()
    {
        return Err("Declare distinct broker capabilities.".into());
    }
    for (requested, required) in [
        (Capability::DocumentEdit, Capability::DocumentRead),
        (Capability::Edit, Capability::Read),
        (Capability::Watch, Capability::Observe),
    ] {
        if has(requested) && !has(required) {
            return Err(format!(
                "{} also requires explicit {}.",
                requested.name(),
                required.name()
            ));
        }
    }
    if provider
        && ![Capability::Observe, Capability::Present]
            .iter()
            .all(|capability| has(*capability))
    {
        return Err("Broker providers require events.observe and ui.present.".into());
    }
    if !provider && has(Capability::Observe) {
        return Err("Background event authority requires a provider declaration.".into());
    }
    Ok(())
}
fn vector<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>, const MAX: usize>(
    decoder: D,
) -> Result<Vec<T>, D::Error> {
    struct Visitor<T, const MAX: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const MAX: usize> serde::de::Visitor<'de> for Visitor<T, MAX> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "at most {MAX} entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() >= MAX {
                    return Err(serde::de::Error::custom("array limit exceeded"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    decoder.deserialize_seq(Visitor::<T, MAX>(std::marker::PhantomData))
}
fn capabilities<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Capability>, D::Error> {
    vector::<_, _, 10>(d)
}
fn arguments<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Argument>, D::Error> {
    vector::<_, _, 32>(d)
}
fn unique_map<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> Result<BTreeMap<String, T>, D::Error> {
    struct Visitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("up to 16 unique named declarations")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                if !options::valid_id(&key) || values.len() >= 16 || values.contains_key(&key) {
                    return Err(serde::de::Error::custom("invalid or duplicate declaration"));
                }
                values.insert(key, map.next_value()?);
            }
            Ok(values)
        }
    }
    decoder.deserialize_map(Visitor::<T>(std::marker::PhantomData))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_do_not_inherit_editor_access_or_approve_resources() {
        let definition:Definition=serde_json::from_str(r#"{"capabilities":["network.https"],"endpoints":{"translation":{"url":"https://example.com/translate","method":"POST","upload":1024,"response":1024}}}"#).unwrap();
        definition.validate(false).unwrap();
        assert!(!definition.has(Capability::DocumentRead));
        for value in [
            r#"{"capabilities":["workspace.edit"]}"#,
            r#"{"capabilities":["document.edit"]}"#,
            r#"{"capabilities":["network.https"]}"#,
            r#"{"capabilities":["network.https"],"endpoints":{"local":{"url":"https://127.0.0.1/","method":"GET","upload":0,"response":32}}}"#,
        ] {
            let definition: Definition = serde_json::from_str(value).unwrap();
            assert!(definition.validate(false).is_err());
        }
        assert!(serde_json::from_str::<Definition>(
            r#"{"capabilities":["ui.present"],"endpoints":{"duplicate":{},"duplicate":{}}}"#
        )
        .is_err());
    }
    #[test]
    fn argv_is_literal_and_rejects_unknown_or_unreviewed_choices() {
        let definition:ToolDefinition=serde_json::from_str(r#"{"protocol":"filter","workspace":false,"arguments":[{"literal":"--emit"},{"choice":"mode"},{"literal":"--"}],"choices":{"mode":["stdout","check"]}}"#).unwrap();
        definition.validate().unwrap();
        let values = hedit_plugin::sdk::Storage(BTreeMap::from([(
            "mode".into(),
            Value::Text("stdout".into()),
        )]));
        assert_eq!(
            definition.argv(&values).unwrap(),
            ["--emit", "stdout", "--"]
        );
        let values = hedit_plugin::sdk::Storage(BTreeMap::from([(
            "mode".into(),
            Value::Text("--config=/private/canary".into()),
        )]));
        assert!(definition.argv(&values).is_err());
        assert!(definition.argv(&Default::default()).is_err());
        let repeated = ToolDefinition {
            protocol: ToolProtocol::Filter,
            workspace: false,
            arguments: vec![Argument::Choice("mode".into()); 32],
            choices: BTreeMap::from([("mode".into(), Choices(vec![Value::Text("x".repeat(256))]))]),
        };
        assert!(repeated.validate().is_err());
    }
}
