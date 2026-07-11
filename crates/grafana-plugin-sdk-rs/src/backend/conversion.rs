//! Kubernetes-style resource conversion for plugins.
//!
//! **Experimental.** Resource conversion lets a plugin convert objects between API
//! versions, following the Kubernetes conversion webhook pattern. Implement
//! [`ConversionService`] and register it with
//! [`Plugin::conversion_service`](crate::backend::Plugin::conversion_service).
use std::fmt;

use serde::de::DeserializeOwned;

use crate::{
    backend::{ConvertFromError, InstanceSettings, PluginContext},
    pluginv2,
};

use super::{admission::StatusResult, GrafanaPlugin, PluginType};

/// The API group and version of a resource.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct GroupVersion {
    /// The API group.
    pub group: String,
    /// The API version.
    pub version: String,
}

impl From<pluginv2::GroupVersion> for GroupVersion {
    fn from(other: pluginv2::GroupVersion) -> Self {
        Self {
            group: other.group,
            version: other.version,
        }
    }
}

/// A resource serialized to bytes together with its media type.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct RawObject {
    /// The serialized object.
    pub raw: Vec<u8>,
    /// The media type of [`raw`](RawObject::raw).
    pub content_type: String,
}

impl RawObject {
    /// Create a `RawObject` from serialized bytes and a content type.
    pub fn new(raw: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self {
            raw,
            content_type: content_type.into(),
        }
    }
}

impl From<pluginv2::RawObject> for RawObject {
    fn from(other: pluginv2::RawObject) -> Self {
        Self {
            raw: other.raw,
            content_type: other.content_type,
        }
    }
}

impl From<RawObject> for pluginv2::RawObject {
    fn from(other: RawObject) -> Self {
        Self {
            raw: other.raw,
            content_type: other.content_type,
        }
    }
}

/// A request to convert one or more objects to a target version.
#[derive(Debug)]
#[non_exhaustive]
pub struct InnerConversionRequest<IS, JsonData, SecureJsonData>
where
    JsonData: fmt::Debug + DeserializeOwned,
    SecureJsonData: DeserializeOwned,
    IS: InstanceSettings<JsonData, SecureJsonData>,
{
    /// Details of the plugin instance from which the request originated.
    ///
    /// Instance settings may be absent depending on the request.
    pub plugin_context: PluginContext<IS, JsonData, SecureJsonData>,
    /// An identifier correlating this request and its response.
    pub uid: String,
    /// The objects to convert.
    pub objects: Vec<RawObject>,
    /// The version to convert the objects to.
    pub target_version: GroupVersion,
}

impl<IS, JsonData, SecureJsonData> TryFrom<pluginv2::ConversionRequest>
    for InnerConversionRequest<IS, JsonData, SecureJsonData>
where
    JsonData: fmt::Debug + DeserializeOwned,
    SecureJsonData: DeserializeOwned,
    IS: InstanceSettings<JsonData, SecureJsonData>,
{
    type Error = ConvertFromError;
    fn try_from(other: pluginv2::ConversionRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            plugin_context: other
                .plugin_context
                .ok_or(ConvertFromError::MissingPluginContext)
                .and_then(TryInto::try_into)?,
            uid: other.uid,
            objects: other.objects.into_iter().map(Into::into).collect(),
            target_version: other.target_version.map(Into::into).unwrap_or_default(),
        })
    }
}

/// A request to convert objects.
///
/// A convenience alias hiding the generics; `T` is the plugin implementation.
pub type ConversionRequest<T> = InnerConversionRequest<
    <<T as GrafanaPlugin>::PluginType as PluginType<
        <T as GrafanaPlugin>::JsonData,
        <T as GrafanaPlugin>::SecureJsonData,
    >>::InstanceSettings,
    <T as GrafanaPlugin>::JsonData,
    <T as GrafanaPlugin>::SecureJsonData,
>;

/// The response to a conversion request.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ConversionResponse {
    /// The `uid` copied from the corresponding request.
    pub uid: String,
    /// Details of why conversion failed, if it did.
    pub result: Option<StatusResult>,
    /// The converted objects (empty on failure).
    pub objects: Vec<RawObject>,
}

impl ConversionResponse {
    /// A successful response returning the converted `objects`.
    #[must_use]
    pub fn new(uid: impl Into<String>, objects: Vec<RawObject>) -> Self {
        Self {
            uid: uid.into(),
            result: Some(StatusResult::success()),
            objects,
        }
    }

    /// A failed response with the given reason.
    #[must_use]
    pub fn failure(uid: impl Into<String>, result: StatusResult) -> Self {
        Self {
            uid: uid.into(),
            result: Some(result),
            objects: Vec::new(),
        }
    }
}

impl From<ConversionResponse> for pluginv2::ConversionResponse {
    fn from(other: ConversionResponse) -> Self {
        Self {
            uid: other.uid,
            result: other.result.map(Into::into),
            objects: other.objects.into_iter().map(Into::into).collect(),
        }
    }
}

/// Trait for plugins that convert resources between versions.
#[tonic::async_trait]
pub trait ConversionService: GrafanaPlugin {
    /// The error type returned when converting objects.
    type Error: std::error::Error;

    /// Convert the request's objects to the target version.
    async fn convert_objects(
        &self,
        request: ConversionRequest<Self>,
    ) -> Result<ConversionResponse, Self::Error>;
}

#[tonic::async_trait]
impl<T> pluginv2::resource_conversion_server::ResourceConversion for T
where
    T: ConversionService + Send + Sync + 'static,
{
    #[tracing::instrument(skip(self), level = "debug")]
    async fn convert_objects(
        &self,
        request: tonic::Request<pluginv2::ConversionRequest>,
    ) -> Result<tonic::Response<pluginv2::ConversionResponse>, tonic::Status> {
        let request = request
            .into_inner()
            .try_into()
            .map_err(ConvertFromError::into_tonic_status)?;
        let response = ConversionService::convert_objects(self, request)
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

    type Request = InnerConversionRequest<AppInstanceSettings<Value, Value>, Value, Value>;

    #[test]
    fn conversion_request_converts_from_proto() {
        let proto = pluginv2::ConversionRequest {
            plugin_context: Some(pluginv2::PluginContext::default()),
            uid: "abc".to_owned(),
            objects: vec![pluginv2::RawObject {
                raw: b"{}".to_vec(),
                content_type: "application/json".to_owned(),
            }],
            target_version: Some(pluginv2::GroupVersion {
                group: "g".to_owned(),
                version: "v2".to_owned(),
            }),
        };
        let request: Request = proto.try_into().unwrap();
        assert_eq!(request.uid, "abc");
        assert_eq!(request.target_version.version, "v2");
        assert_eq!(request.objects.len(), 1);
        assert_eq!(request.objects[0].content_type, "application/json");
    }

    #[test]
    fn conversion_response_converts_to_proto() {
        let response: pluginv2::ConversionResponse = ConversionResponse::new(
            "abc",
            vec![RawObject::new(b"{}".to_vec(), "application/json")],
        )
        .into();
        assert_eq!(response.uid, "abc");
        assert_eq!(response.result.unwrap().status, "Success");
        assert_eq!(response.objects.len(), 1);
    }
}
