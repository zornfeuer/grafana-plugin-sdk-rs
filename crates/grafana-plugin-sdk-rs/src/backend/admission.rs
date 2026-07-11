//! Kubernetes-style admission control for plugins.
//!
//! **Experimental.** Admission control lets a plugin validate or mutate resources
//! before they are persisted, following the Kubernetes admission webhook pattern.
//! Implement [`AdmissionService`] and register it with
//! [`Plugin::admission_service`](crate::backend::Plugin::admission_service).
use std::fmt;

use serde::de::DeserializeOwned;

use crate::{
    backend::{ConvertFromError, InstanceSettings, PluginContext},
    pluginv2,
};

use super::{GrafanaPlugin, PluginType};

/// A machine- and human-readable status, modelled on Kubernetes' `Status`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct StatusResult {
    /// The status of the operation: `"Success"` or `"Failure"`.
    pub status: String,
    /// A human-readable description of the status.
    pub message: String,
    /// A machine-readable reason for a failure, if any.
    pub reason: String,
    /// A suggested HTTP status code (`0` if unset).
    pub code: i32,
}

impl StatusResult {
    /// A `"Failure"` status with the given message.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            status: "Failure".to_owned(),
            message: message.into(),
            ..Default::default()
        }
    }

    /// A `"Success"` status.
    #[must_use]
    pub fn success() -> Self {
        Self {
            status: "Success".to_owned(),
            ..Default::default()
        }
    }
}

impl From<StatusResult> for pluginv2::StatusResult {
    fn from(other: StatusResult) -> Self {
        Self {
            status: other.status,
            message: other.message,
            reason: other.reason,
            code: other.code,
        }
    }
}

/// The group, version and kind identifying a resource type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GroupVersionKind {
    /// The API group.
    pub group: String,
    /// The API version.
    pub version: String,
    /// The resource kind.
    pub kind: String,
}

impl From<pluginv2::GroupVersionKind> for GroupVersionKind {
    fn from(other: pluginv2::GroupVersionKind) -> Self {
        Self {
            group: other.group,
            version: other.version,
            kind: other.kind,
        }
    }
}

/// The type of resource operation being admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdmissionOperation {
    /// The object is being created.
    Create,
    /// The object is being updated.
    Update,
    /// The object is being deleted.
    Delete,
}

impl From<pluginv2::admission_request::Operation> for AdmissionOperation {
    fn from(other: pluginv2::admission_request::Operation) -> Self {
        match other {
            pluginv2::admission_request::Operation::Create => Self::Create,
            pluginv2::admission_request::Operation::Update => Self::Update,
            pluginv2::admission_request::Operation::Delete => Self::Delete,
        }
    }
}

/// A request to admit (validate or mutate) a resource.
#[derive(Debug)]
#[non_exhaustive]
pub struct InnerAdmissionRequest<IS, JsonData, SecureJsonData>
where
    JsonData: fmt::Debug + DeserializeOwned,
    SecureJsonData: DeserializeOwned,
    IS: InstanceSettings<JsonData, SecureJsonData>,
{
    /// Details of the plugin instance from which the request originated.
    ///
    /// Instance settings may be absent depending on the request.
    pub plugin_context: PluginContext<IS, JsonData, SecureJsonData>,
    /// The operation being performed.
    pub operation: AdmissionOperation,
    /// The kind of the object in the request.
    pub kind: GroupVersionKind,
    /// The object in the request, including its full metadata envelope.
    pub object_bytes: Vec<u8>,
    /// The object as it currently exists in storage (for updates/deletes).
    pub old_object_bytes: Vec<u8>,
}

impl<IS, JsonData, SecureJsonData> TryFrom<pluginv2::AdmissionRequest>
    for InnerAdmissionRequest<IS, JsonData, SecureJsonData>
where
    JsonData: fmt::Debug + DeserializeOwned,
    SecureJsonData: DeserializeOwned,
    IS: InstanceSettings<JsonData, SecureJsonData>,
{
    type Error = ConvertFromError;
    fn try_from(other: pluginv2::AdmissionRequest) -> Result<Self, Self::Error> {
        let operation = other.operation().into();
        Ok(Self {
            plugin_context: other
                .plugin_context
                .ok_or(ConvertFromError::MissingPluginContext)
                .and_then(TryInto::try_into)?,
            operation,
            kind: other.kind.map(Into::into).unwrap_or_default(),
            object_bytes: other.object_bytes,
            old_object_bytes: other.old_object_bytes,
        })
    }
}

/// A request to admit a resource.
///
/// A convenience alias hiding the generics; `T` is the plugin implementation.
pub type AdmissionRequest<T> = InnerAdmissionRequest<
    <<T as GrafanaPlugin>::PluginType as PluginType<
        <T as GrafanaPlugin>::JsonData,
        <T as GrafanaPlugin>::SecureJsonData,
    >>::InstanceSettings,
    <T as GrafanaPlugin>::JsonData,
    <T as GrafanaPlugin>::SecureJsonData,
>;

/// The response to a validating admission request.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ValidationResponse {
    /// Whether the request is permitted.
    pub allowed: bool,
    /// Details of why the request was denied (ignored when `allowed`).
    pub result: Option<StatusResult>,
    /// Warnings to return to the API client.
    pub warnings: Vec<String>,
}

impl ValidationResponse {
    /// An allowed response.
    #[must_use]
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            ..Default::default()
        }
    }

    /// A denied response with the given reason.
    #[must_use]
    pub fn denied(result: StatusResult) -> Self {
        Self {
            allowed: false,
            result: Some(result),
            ..Default::default()
        }
    }

    /// Attach warnings to the response.
    #[must_use]
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }
}

impl From<ValidationResponse> for pluginv2::ValidationResponse {
    fn from(other: ValidationResponse) -> Self {
        Self {
            allowed: other.allowed,
            result: other.result.map(Into::into),
            warnings: other.warnings,
        }
    }
}

/// The response to a mutating admission request.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct MutationResponse {
    /// Whether the request is permitted.
    pub allowed: bool,
    /// Details of why the request was denied (ignored when `allowed`).
    pub result: Option<StatusResult>,
    /// Warnings to return to the API client.
    pub warnings: Vec<String>,
    /// The mutated object bytes.
    pub object_bytes: Vec<u8>,
}

impl MutationResponse {
    /// An allowed response returning the (possibly mutated) object.
    #[must_use]
    pub fn allowed(object_bytes: Vec<u8>) -> Self {
        Self {
            allowed: true,
            object_bytes,
            ..Default::default()
        }
    }

    /// A denied response with the given reason.
    #[must_use]
    pub fn denied(result: StatusResult) -> Self {
        Self {
            allowed: false,
            result: Some(result),
            ..Default::default()
        }
    }
}

impl From<MutationResponse> for pluginv2::MutationResponse {
    fn from(other: MutationResponse) -> Self {
        Self {
            allowed: other.allowed,
            result: other.result.map(Into::into),
            warnings: other.warnings,
            object_bytes: other.object_bytes,
        }
    }
}

/// Trait for plugins that validate and/or mutate resources via admission control.
#[tonic::async_trait]
pub trait AdmissionService: GrafanaPlugin {
    /// The error type returned when validating a resource.
    type ValidationError: std::error::Error;

    /// Validate a resource; the response is essentially a yes/no with details.
    async fn validate_admission(
        &self,
        request: AdmissionRequest<Self>,
    ) -> Result<ValidationResponse, Self::ValidationError>;

    /// The error type returned when mutating a resource.
    type MutationError: std::error::Error;

    /// Return a (possibly mutated) copy of the object that can be saved.
    async fn mutate_admission(
        &self,
        request: AdmissionRequest<Self>,
    ) -> Result<MutationResponse, Self::MutationError>;
}

#[tonic::async_trait]
impl<T> pluginv2::admission_control_server::AdmissionControl for T
where
    T: AdmissionService + Send + Sync + 'static,
{
    #[tracing::instrument(skip(self), level = "debug")]
    async fn validate_admission(
        &self,
        request: tonic::Request<pluginv2::AdmissionRequest>,
    ) -> Result<tonic::Response<pluginv2::ValidationResponse>, tonic::Status> {
        let request = request
            .into_inner()
            .try_into()
            .map_err(ConvertFromError::into_tonic_status)?;
        let response = AdmissionService::validate_admission(self, request)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;
        Ok(tonic::Response::new(response.into()))
    }

    #[tracing::instrument(skip(self), level = "debug")]
    async fn mutate_admission(
        &self,
        request: tonic::Request<pluginv2::AdmissionRequest>,
    ) -> Result<tonic::Response<pluginv2::MutationResponse>, tonic::Status> {
        let request = request
            .into_inner()
            .try_into()
            .map_err(ConvertFromError::into_tonic_status)?;
        let response = AdmissionService::mutate_admission(self, request)
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;
        Ok(tonic::Response::new(response.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppInstanceSettings;
    use serde_json::Value;

    type Request = InnerAdmissionRequest<AppInstanceSettings<Value, Value>, Value, Value>;

    #[test]
    fn admission_request_converts_from_proto() {
        let proto = pluginv2::AdmissionRequest {
            plugin_context: Some(pluginv2::PluginContext::default()),
            operation: pluginv2::admission_request::Operation::Update as i32,
            kind: Some(pluginv2::GroupVersionKind {
                group: "g".to_owned(),
                version: "v1".to_owned(),
                kind: "Dashboard".to_owned(),
            }),
            object_bytes: b"new".to_vec(),
            old_object_bytes: b"old".to_vec(),
        };
        let request: Request = proto.try_into().unwrap();
        assert_eq!(request.operation, AdmissionOperation::Update);
        assert_eq!(request.kind.kind, "Dashboard");
        assert_eq!(request.object_bytes, b"new");
        assert_eq!(request.old_object_bytes, b"old");
    }

    #[test]
    fn responses_convert_to_proto() {
        let validation: pluginv2::ValidationResponse =
            ValidationResponse::denied(StatusResult::failure("nope"))
                .with_warnings(vec!["careful".to_owned()])
                .into();
        assert!(!validation.allowed);
        assert_eq!(validation.result.unwrap().message, "nope");
        assert_eq!(validation.warnings, vec!["careful".to_owned()]);

        let mutation: pluginv2::MutationResponse =
            MutationResponse::allowed(b"mutated".to_vec()).into();
        assert!(mutation.allowed);
        assert_eq!(mutation.object_bytes, b"mutated");
    }
}
