use crate::pb;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

impl From<Pod> for pb::Pod {
    fn from(f: Pod) -> Self {
        Self {
            metadata: Some(f.metadata.into()),
        }
    }
}

impl From<(String, String)> for pb::Annotation {
    fn from((k, v): (String, String)) -> Self {
        Self {
            key: Some(k),
            value: Some(v),
        }
    }
}

impl From<(String, String)> for pb::Label {
    fn from((k, v): (String, String)) -> Self {
        Self {
            key: Some(k),
            value: Some(v),
        }
    }
}

impl From<ObjectMeta> for pb::ObjectMeta {
    fn from(f: ObjectMeta) -> Self {
        Self {
            namespace: f.namespace,
            name: f.name,
            annotations: f
                .annotations
                .map(|a| a.into_iter().map_into().collect())
                .unwrap_or_default(),
            labels: f
                .labels
                .map(|a| a.into_iter().map_into().collect())
                .unwrap_or_default(),
        }
    }
}
