//! JSON-RPC 2.0 types for MCP communication.
//!
//! These types are intentionally independent of `helix-lsp::jsonrpc` to avoid
//! a dependency on `helix-lsp` and because the MCP wire format differs slightly.
//! Follows the same patterns as `helix-lsp/src/jsonrpc.rs` for consistency.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol version identifier.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum Version {
    V2,
}

impl serde::Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {
            Version::V2 => serializer.serialize_str("2.0"),
        }
    }
}

struct VersionVisitor;

impl<'de> serde::de::Visitor<'de> for VersionVisitor {
    type Value = Version;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON-RPC version string")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match value {
            "2.0" => Ok(Version::V2),
            _ => Err(serde::de::Error::custom("invalid JSON-RPC version")),
        }
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_identifier(VersionVisitor)
    }
}

/// JSON-RPC request/response identifier.
///
/// Per the spec, IDs can be numbers, strings, or null. Numbers SHOULD NOT
/// contain fractional parts, but we accept floats representing whole numbers
/// for interop with JavaScript clients.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum Id {
    Number(i64),
    String(String),
    Null,
}

impl serde::Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Id::Number(n) => serializer.serialize_i64(*n),
            Id::String(s) => serializer.serialize_str(s),
            Id::Null => serializer.serialize_unit(),
        }
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct IdVisitor;

        impl<'de> serde::de::Visitor<'de> for IdVisitor {
            type Value = Id;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a JSON-RPC id (number, string, or null)")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Id::Number(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let v: i64 = v
                    .try_into()
                    .map_err(|_| E::custom("id number out of i64 range"))?;
                Ok(Id::Number(v))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.is_sign_positive() && v.fract() == 0.0 && v <= i64::MAX as f64 {
                    Ok(Id::Number(v as i64))
                } else {
                    Err(E::custom("id float must represent a positive whole number"))
                }
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Id::String(v.to_owned()))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Id::Null)
            }
        }

        deserializer.deserialize_any(IdVisitor)
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Id::Number(n) => write!(f, "{}", n),
            Id::String(s) => f.write_str(s),
            Id::Null => f.write_str("null"),
        }
    }
}

/// A JSON-RPC 2.0 method call (request).
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct MethodCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<Version>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    pub id: Id,
}

/// A JSON-RPC 2.0 notification (no `id` field).
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Notification {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<Version>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// An incoming JSON-RPC call, either a method call or a notification.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Call {
    MethodCall(MethodCall),
    Notification(Notification),
}

impl From<MethodCall> for Call {
    fn from(mc: MethodCall) -> Self {
        Call::MethodCall(mc)
    }
}

impl From<Notification> for Call {
    fn from(n: Notification) -> Self {
        Call::Notification(n)
    }
}

/// A JSON-RPC request: single call or batch.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Request {
    Single(Call),
    Batch(Vec<Call>),
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Error {
    /// Create an error for invalid parameters.
    pub fn invalid_params<M>(message: M) -> Self
    where
        M: Into<String>,
    {
        Error {
            code: crate::protocol::INVALID_PARAMS,
            message: message.into(),
            data: None,
        }
    }

    /// Create an error for a method not found.
    pub fn method_not_found<M>(message: M) -> Self
    where
        M: Into<String>,
    {
        Error {
            code: crate::protocol::METHOD_NOT_FOUND,
            message: message.into(),
            data: None,
        }
    }

    /// Create an internal error.
    pub fn internal_error<M>(message: M) -> Self
    where
        M: Into<String>,
    {
        Error {
            code: crate::protocol::INTERNAL_ERROR,
            message: message.into(),
            data: None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// A successful JSON-RPC response.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Success {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<Version>,
    pub result: Value,
    pub id: Id,
}

/// A failed JSON-RPC response.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Failure {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<Version>,
    pub error: Error,
    pub id: Id,
}

/// A JSON-RPC response output: either success or failure.
///
/// Note: `Failure` comes first so that a response containing both
/// `result` and `error` is deserialized as a `Failure`.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Output {
    Failure(Failure),
    Success(Success),
}

impl From<Output> for Result<Value, Error> {
    fn from(output: Output) -> Self {
        match output {
            Output::Success(s) => Ok(s.result),
            Output::Failure(f) => Err(f.error),
        }
    }
}

/// A JSON-RPC response: single output or batch.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Single(Output),
    Batch(Vec<Output>),
}

impl From<Failure> for Response {
    fn from(f: Failure) -> Self {
        Response::Single(Output::Failure(f))
    }
}

impl From<Success> for Response {
    fn from(s: Success) -> Self {
        Response::Single(Output::Success(s))
    }
}

/// Build a JSON-RPC success response for a given id and result.
pub fn success(id: Id, result: Value) -> Output {
    Output::Success(Success {
        jsonrpc: Some(Version::V2),
        result,
        id,
    })
}

/// Build a JSON-RPC error response for a given id and error.
pub fn failure(id: Id, error: Error) -> Output {
    Output::Failure(Failure {
        jsonrpc: Some(Version::V2),
        error,
        id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_call_serialize() {
        let m = MethodCall {
            jsonrpc: Some(Version::V2),
            method: "initialize".to_owned(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05"
            })),
            id: Id::Number(1),
        };

        let serialized = serde_json::to_string(&m).unwrap();
        assert_eq!(
            serialized,
            r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}"#
        );
    }

    #[test]
    fn notification_serialize() {
        let n = Notification {
            jsonrpc: Some(Version::V2),
            method: "initialized".to_owned(),
            params: None,
        };

        let serialized = serde_json::to_string(&n).unwrap();
        assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"initialized"}"#);
    }

    #[test]
    fn notification_with_params_serialize() {
        let n = Notification {
            jsonrpc: Some(Version::V2),
            method: "notifications/initialized".to_owned(),
            params: Some(serde_json::json!({})),
        };

        let serialized = serde_json::to_string(&n).unwrap();
        assert_eq!(
            serialized,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#
        );
    }

    #[test]
    fn id_deserialize_number() {
        let deserialized: Id = serde_json::from_str("42").unwrap();
        assert_eq!(deserialized, Id::Number(42));
    }

    #[test]
    fn id_deserialize_string() {
        let deserialized: Id = serde_json::from_str(r#""my-id""#).unwrap();
        assert_eq!(deserialized, Id::String("my-id".to_owned()));
    }

    #[test]
    fn id_deserialize_null() {
        let deserialized: Id = serde_json::from_str("null").unwrap();
        assert_eq!(deserialized, Id::Null);
    }

    #[test]
    fn success_output_deserialize() {
        let json = r#"{"jsonrpc":"2.0","result":{"key":"value"},"id":1}"#;
        let deserialized: Output = serde_json::from_str(json).unwrap();
        match deserialized {
            Output::Success(s) => {
                assert_eq!(s.id, Id::Number(1));
                assert_eq!(s.result, serde_json::json!({"key": "value"}));
            }
            _ => panic!("expected Success"),
        }
    }

    #[test]
    fn failure_output_deserialize() {
        let json =
            r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#;
        let deserialized: Output = serde_json::from_str(json).unwrap();
        match deserialized {
            Output::Failure(f) => {
                assert_eq!(f.id, Id::Number(1));
                assert_eq!(f.error.code, -32601);
            }
            _ => panic!("expected Failure"),
        }
    }

    #[test]
    fn call_method_call_deserialize() {
        let json = r#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#;
        let deserialized: Call = serde_json::from_str(json).unwrap();
        match deserialized {
            Call::MethodCall(mc) => {
                assert_eq!(mc.method, "tools/list");
                assert_eq!(mc.id, Id::Number(1));
                assert!(mc.params.is_none());
            }
            _ => panic!("expected MethodCall"),
        }
    }

    #[test]
    fn call_notification_deserialize() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let deserialized: Call = serde_json::from_str(json).unwrap();
        match deserialized {
            Call::Notification(n) => {
                assert_eq!(n.method, "notifications/initialized");
            }
            _ => panic!("expected Notification"),
        }
    }
}
